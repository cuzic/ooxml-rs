//! Shared manual-check and python-pptx fixtures (ADR-0016 §§9–10).

use ooxml_pptx::{
    LineSpec, Presentation, Result, RunFill, TablePartStyle, TableStyleBorders, TableStyleCell,
    TableStyleDef, TableStyleParts, TableStyleText, ThemeColor, ThemeColorSlot, ThemeFont,
    ThemeFonts, WriteColor,
};

pub const TABLE_FIXTURE: &str = "poi/table_test2.pptx";
pub const TABLE_NAME: &str = "34-p3e-custom-table-style";
pub const THEME_FIXTURE: &str = "poi/sample.pptx";
pub const THEME_NAME: &str = "35-p3e-edit-master-theme";

pub fn definition() -> TableStyleDef {
    TableStyleDef {
        name: "Teal header".into(),
        parts: TableStyleParts {
            whole_table: Some(TablePartStyle {
                cell: Some(TableStyleCell {
                    fill: Some(RunFill::Solid(WriteColor::Rgb([232, 245, 245]).into())),
                    borders: TableStyleBorders {
                        inside_horizontal: Some(LineSpec {
                            width: Some(12_700),
                            fill: Some(RunFill::Solid(WriteColor::Rgb([130, 162, 164]).into())),
                            ..LineSpec::default()
                        }),
                        ..TableStyleBorders::default()
                    },
                }),
                ..TablePartStyle::default()
            }),
            first_row: Some(TablePartStyle {
                text: Some(TableStyleText {
                    bold: Some(true),
                    italic: Some(false),
                    font: Some(ThemeFont::Major),
                    color: Some(WriteColor::Rgb([255, 255, 255]).into()),
                }),
                cell: Some(TableStyleCell {
                    fill: Some(RunFill::Solid(WriteColor::Rgb([28, 132, 148]).into())),
                    ..TableStyleCell::default()
                }),
            }),
            ..TableStyleParts::default()
        },
    }
}

pub fn edit_table(deck: &mut Presentation) -> Result<()> {
    let id = deck.add_table_style(definition())?;
    let mut slide = deck.slide_mut(0)?;
    let mut shape = slide.shape_mut_by_id(4)?;
    let mut table = shape.table_mut()?;
    table.set_flag(ooxml_pptx::TableFlag::FirstRow, true)?;
    table.set_style(Some(&id))
}

pub fn edit_theme(deck: &mut Presentation) -> Result<()> {
    let mut theme = deck.master_theme_mut(0)?;
    theme.set_color(ThemeColorSlot::Accent1, ThemeColor::Rgb([28, 132, 148]))?;
    theme.set_major_font(ThemeFonts {
        latin: "Georgia",
        east_asian: "Yu Mincho",
        complex_script: "Arial",
    })
}
