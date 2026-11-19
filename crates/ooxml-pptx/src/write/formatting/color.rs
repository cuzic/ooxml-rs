//! Colour choices and transforms for direct formatting.

use super::*;
pub use dml::{
    PresetColorValues as PresetColor, SchemeColorValues as SchemeColor,
    SystemColorValues as SystemColor,
};
use ooxml_core::EnumType;

/// A DrawingML colour. Dark and light scheme slots bypass the slide colour map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    /// An sRGB triplet.
    Rgb([u8; 3]),
    /// A slot in the theme colour scheme.
    Scheme(SchemeColor),
    /// A system colour with an optional fallback RGB value.
    System(SystemColor, Option<[u8; 3]>),
    /// A named DrawingML colour.
    Preset(PresetColor),
}

impl Color {
    /// The primary or secondary text colour, mapped by the slide's colour map.
    pub fn scheme_for_text(secondary: bool) -> Self {
        Self::Scheme(if secondary {
            SchemeColor::Text2
        } else {
            SchemeColor::Text1
        })
    }
}

/// A colour and its ordered DrawingML transforms.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSpec {
    /// The base colour.
    pub color: Color,
    /// Transforms in evaluation order.
    pub transforms: Vec<ColorTransform>,
}

impl From<Color> for ColorSpec {
    fn from(color: Color) -> Self {
        Self {
            color,
            transforms: Vec::new(),
        }
    }
}

macro_rules! color_transforms {
    (
        percentages { $($percent:ident, $percent_view:ident => $percent_xml:literal, $min_percent:expr, $max_percent:expr;)* }
        angles { $($angle:ident, $angle_view:ident => $angle_xml:literal, $min_angle:expr, $max_angle:expr;)* }
        unvalued { $($empty:ident => $empty_xml:literal,)* }
    ) => {
        /// A colour transform. Percentages use thousandths of a percent and
        /// angles use 60,000ths of a degree.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ColorTransform {
            $(
                #[doc = concat!("DrawingML `", $percent_xml, "`.")]
                $percent(i32),
            )*
            $(
                #[doc = concat!("DrawingML `", $angle_xml, "`.")]
                $angle(i32),
            )*
            $(
                #[doc = concat!("DrawingML `", $empty_xml, "`.")]
                $empty,
            )*
        }

        impl ColorTransform {
            fn validate(self) -> Result<()> {
                match self {
                    $(Self::$percent(value) => check_range(value, $min_percent, $max_percent, $percent_xml),)*
                    $(Self::$angle(value) => check_range(value, $min_angle, $max_angle, $angle_xml),)*
                    $(Self::$empty => Ok(()),)*
                }
            }

            fn matches(self, node: &RawNode) -> bool {
                match self {
                    $(
                        Self::$percent(value) => node.is(dml::$percent::QNAME)
                            && node.attr(dml::$percent::ATTR_VAL).and_then(parse_dml_percent)
                                .is_some_and(|parsed| parsed.percent() == DmlPercent::Thousandths(value).percent()),
                    )*
                    $(
                        Self::$angle(value) => node.is(dml::$angle::QNAME)
                            && node.attr(dml::$angle::ATTR_VAL).and_then(int) == Some(value),
                    )*
                    $(Self::$empty => node.is(dml::$empty::QNAME),)*
                }
            }

            fn write<E: ElementSpec>(self, parent: &mut LensMut<'_, E>) -> Result<()>
            where
                $(dml::$percent: ChildOf<E>,)*
                $(dml::$angle: ChildOf<E>,)*
                $(dml::$empty: ChildOf<E>,)*
            {
                match self {
                    $(Self::$percent(value) => dml::$percent_view(parent.insert::<dml::$percent>()?).set_val(value)?,)*
                    $(Self::$angle(value) => dml::$angle_view(parent.insert::<dml::$angle>()?).set_val(value)?,)*
                    $(Self::$empty => { parent.insert::<dml::$empty>()?; },)*
                }
                Ok(())
            }
        }
    };
}

color_transforms! {
    percentages {
        Tint, TintViewMut => "tint", 0, 100_000;
        Shade, ShadeViewMut => "shade", 0, 100_000;
        Alpha, AlphaViewMut => "alpha", 0, 100_000;
        AlphaOffset, AlphaOffsetViewMut => "alphaOff", -100_000, 100_000;
        AlphaModulation, AlphaModulationViewMut => "alphaMod", 0, 2_147_483_647;
        HueModulation, HueModulationViewMut => "hueMod", 0, 2_147_483_647;
        Saturation, SaturationViewMut => "sat", i32::MIN, i32::MAX;
        SaturationOffset, SaturationOffsetViewMut => "satOff", i32::MIN, i32::MAX;
        SaturationModulation, SaturationModulationViewMut => "satMod", i32::MIN, i32::MAX;
        Luminance, LuminanceViewMut => "lum", i32::MIN, i32::MAX;
        LuminanceOffset, LuminanceOffsetViewMut => "lumOff", i32::MIN, i32::MAX;
        LuminanceModulation, LuminanceModulationViewMut => "lumMod", i32::MIN, i32::MAX;
        Red, RedViewMut => "red", i32::MIN, i32::MAX;
        RedOffset, RedOffsetViewMut => "redOff", i32::MIN, i32::MAX;
        RedModulation, RedModulationViewMut => "redMod", i32::MIN, i32::MAX;
        Green, GreenViewMut => "green", i32::MIN, i32::MAX;
        GreenOffset, GreenOffsetViewMut => "greenOff", i32::MIN, i32::MAX;
        GreenModulation, GreenModulationViewMut => "greenMod", i32::MIN, i32::MAX;
        Blue, BlueViewMut => "blue", i32::MIN, i32::MAX;
        BlueOffset, BlueOffsetViewMut => "blueOff", i32::MIN, i32::MAX;
        BlueModulation, BlueModulationViewMut => "blueMod", i32::MIN, i32::MAX;
    }
    angles {
        Hue, HueViewMut => "hue", 0, 21_599_999;
        HueOffset, HueOffsetViewMut => "hueOff", i32::MIN, i32::MAX;
    }
    unvalued {
        Complement => "comp",
        Inverse => "inv",
        Gray => "gray",
        Gamma => "gamma",
        InverseGamma => "invGamma",
    }
}

const COLOR_CHOICES: [QName; 6] = [
    a("srgbClr"),
    a("scrgbClr"),
    a("hslClr"),
    a("schemeClr"),
    a("sysClr"),
    a("prstClr"),
];

impl ColorSpec {
    pub(super) fn validate(&self) -> Result<()> {
        if self.color == Color::Scheme(SchemeColor::PhColor) {
            return Err(Error::InvalidSchemeColor);
        }
        self.transforms
            .iter()
            .try_for_each(|transform| transform.validate())
    }

    pub(super) fn matches(&self, parent: &RawNode) -> bool {
        let Some(child) = only_choice(Some(parent), &COLOR_CHOICES) else {
            return false;
        };
        let raw = |name| child.attr(QName::new("", name));
        let base_matches = match self.color {
            Color::Rgb(rgb) => {
                child.is(a("srgbClr"))
                    && raw("val").is_some_and(|s| s.eq_ignore_ascii_case(&hex(rgb)))
            }
            Color::Scheme(color) => {
                child.is(a("schemeClr")) && raw("val").and_then(SchemeColor::parse) == Some(color)
            }
            Color::Preset(color) => {
                child.is(a("prstClr")) && raw("val").and_then(PresetColor::parse) == Some(color)
            }
            Color::System(color, last) => {
                child.is(a("sysClr"))
                    && raw("val").and_then(SystemColor::parse) == Some(color)
                    && match last {
                        Some(rgb) => {
                            raw("lastClr").is_some_and(|s| s.eq_ignore_ascii_case(&hex(rgb)))
                        }
                        None => raw("lastClr").is_none(),
                    }
            }
        };
        base_matches
            && child.elements().count() == self.transforms.len()
            && child
                .elements()
                .zip(&self.transforms)
                .all(|(node, transform)| transform.matches(node))
    }

    pub(super) fn write<E: ElementSpec>(&self, parent: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::RgbColorModelHex: ChildOf<E>,
        dml::SchemeColor: ChildOf<E>,
        dml::SystemColor: ChildOf<E>,
        dml::PresetColor: ChildOf<E>,
    {
        macro_rules! write_color {
            ($spec:ty, $view:path, $value:expr $(, $last:expr)?) => {{
                let mut view = $view(parent.insert::<$spec>()?);
                view.set_val($value)?;
                $(if let Some(rgb) = $last { view.set_last_color(&hex(rgb))?; })?
                for &transform in &self.transforms {
                    transform.write(&mut view.0)?;
                }
            }};
        }
        match self.color {
            Color::Rgb(rgb) => write_color!(
                dml::RgbColorModelHex,
                dml::RgbColorModelHexViewMut,
                &hex(rgb)
            ),
            Color::Scheme(color) => write_color!(dml::SchemeColor, dml::SchemeColorViewMut, color),
            Color::System(color, last) => {
                write_color!(dml::SystemColor, dml::SystemColorViewMut, color, last)
            }
            Color::Preset(color) => write_color!(dml::PresetColor, dml::PresetColorViewMut, color),
        }
        Ok(())
    }
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("{r:02X}{g:02X}{b:02X}")
}
