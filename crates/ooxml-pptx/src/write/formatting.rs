//! Direct text formatting (ADR-0016).

use super::*;
use ooxml_core::value::{
    DmlPercent, first_invalid_xml_char, parse_bool, parse_dml_percent, parse_i32 as int,
    trim_xml_ws,
};
use ooxml_schema::dml::{
    TextAlignmentTypeValues, TextCapsValues, TextFontAlignmentValues, TextStrikeValues,
    TextUnderlineValues,
};

fn remove_choices<E: ElementSpec>(lens: &mut LensMut<'_, E>, choices: &[QName]) {
    while let Some(index) = {
        lens.node()
            .indexed_elements()
            .find_map(|(index, child)| choices.iter().any(|&name| child.is(name)).then_some(index))
    } {
        lens.remove_at(index);
    }
}

/// A direct formatting operation.
#[derive(Clone, Debug, PartialEq)]
pub enum FormatChange<T> {
    /// Writes a direct value.
    Set(T),
    /// Removes the direct value.
    Clear,
}

fn check_range(value: i32, min: i32, max: i32, attr: &'static str) -> Result<()> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(Error::ValueOutOfRange { what: attr })
    }
}

fn attr_equals<T: PartialEq>(
    node: Option<&RawNode>,
    attr: QName,
    change: &FormatChange<T>,
    parse: impl FnOnce(&str) -> Option<T>,
) -> bool {
    let raw = node.and_then(|node| node.attr(attr));
    match change {
        FormatChange::Set(value) => raw.and_then(parse).as_ref() == Some(value),
        FormatChange::Clear => raw.is_none(),
    }
}

fn check_attribute_text(value: &str) -> Result<()> {
    if let Some((offset, ch)) = first_invalid_xml_char(value) {
        return Err(Error::InvalidXmlChar { offset, ch });
    }
    Ok(())
}

macro_rules! run_attr_equals {
    ($node:expr, $element:ident, ATTR_BASELINE, $change:expr, $parse:expr) => {{
        let raw = $node.and_then(|node| node.attr($element::ATTR_BASELINE));
        match $change {
            FormatChange::Set(value) => raw
                .and_then($parse)
                .is_some_and(|old| old.percent() == value.percent()),
            FormatChange::Clear => raw.is_none(),
        }
    }};
    ($node:expr, $element:ident, $attr:ident, $change:expr, $parse:expr) => {
        attr_equals($node, $element::$attr, $change, $parse)
    };
}

fn enum_value<T: ooxml_core::EnumType>(raw: &str) -> Option<T> {
    T::parse(raw)
}

mod autofit;
mod color;
mod paragraph;
mod range;
mod underline;
pub use autofit::AutoFitMode;
pub use color::{
    Color as WriteColor, ColorSpec, ColorTransform, PresetColor, SchemeColor, SystemColor,
};
pub use paragraph::{BulletSize, BulletSpec, TabStop};
pub use underline::{LineEnd, LineSpec, UnderlineFill, UnderlineLine};

/// A DrawingML font slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Script {
    /// Latin font.
    Latin,
    /// East Asian font.
    EastAsian,
    /// Complex-script font.
    ComplexScript,
    /// Symbol font.
    Symbol,
}

/// A theme font family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeFont {
    /// The major font family.
    Major,
    /// The minor font family.
    Minor,
}

/// An explicit font name or a theme font reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontRef {
    /// Named typeface.
    Typeface(String),
    /// Theme family and script slot.
    Theme(ThemeFont, Script),
}

impl FontRef {
    fn typeface(&self) -> Result<&str> {
        use Script::{ComplexScript, EastAsian, Latin};
        use ThemeFont::{Major, Minor};
        Ok(match self {
            Self::Typeface(name) => name,
            Self::Theme(Major, Latin) => "+mj-lt",
            Self::Theme(Major, EastAsian) => "+mj-ea",
            Self::Theme(Major, ComplexScript) => "+mj-cs",
            Self::Theme(Minor, Latin) => "+mn-lt",
            Self::Theme(Minor, EastAsian) => "+mn-ea",
            Self::Theme(Minor, ComplexScript) => "+mn-cs",
            Self::Theme(_, Script::Symbol) => {
                return Err(Error::ValueOutOfRange { what: "typeface" });
            }
        })
    }
}

/// The direct run fill.
#[derive(Clone, Debug, PartialEq)]
pub enum RunFill {
    /// Solid colour and transforms.
    Solid(ColorSpec),
    /// Suppress the text fill.
    NoFill,
}

const FILL: [QName; 6] = [
    a("noFill"),
    a("solidFill"),
    a("gradFill"),
    a("blipFill"),
    a("pattFill"),
    a("grpFill"),
];

fn only_choice<'a>(node: Option<&'a RawNode>, names: &[QName]) -> Option<&'a RawNode> {
    let mut children = node?
        .elements()
        .filter(|node| names.iter().any(|&name| node.is(name)));
    let first = children.next()?;
    children.next().is_none().then_some(first)
}

fn changes_choice<T>(
    node: Option<&RawNode>,
    names: &[QName],
    change: &Option<FormatChange<T>>,
    matches: impl FnOnce(&T, &RawNode) -> bool,
) -> bool {
    match change {
        None => false,
        Some(FormatChange::Clear) => node.is_some_and(|node| {
            node.elements()
                .any(|child| names.iter().any(|&name| child.is(name)))
        }),
        Some(FormatChange::Set(value)) => {
            only_choice(node, names).is_none_or(|child| !matches(value, child))
        }
    }
}

fn fill_matches(fill: &RunFill, child: &RawNode) -> bool {
    match fill {
        RunFill::Solid(color) => child.is(dml::SolidFill::QNAME) && color.matches(child),
        RunFill::NoFill => child.is(dml::NoFill::QNAME),
    }
}

fn font_matches(font: &FontRef, child: &RawNode) -> bool {
    font.typeface().ok() == child.attr(QName::new("", "typeface"))
}

macro_rules! write_font {
    ($parent:expr, $change:expr, $element:ident, $view:ident) => {
        if changes_choice(
            Some($parent.node()),
            &[dml::$element::QNAME],
            $change,
            font_matches,
        ) {
            if let Some(FormatChange::Set(font)) = $change {
                let mut view = dml::$view($parent.get_or_insert::<dml::$element>()?);
                view.set_typeface(font.typeface()?)?;
                view.remove_panose()?;
                view.remove_pitch_family()?;
                view.remove_character_set()?;
            } else {
                $parent.remove::<dml::$element>();
            }
        }
    };
}

const UNDERLINE_APPEARANCE: [QName; 4] = [a("uLn"), a("uLnTx"), a("uFill"), a("uFillTx")];

macro_rules! apply_children {
    ($format:expr, $parent:expr) => {
        let parent = &mut $parent;
        if matches!($format.underline, Some(FormatChange::Clear)) {
            remove_choices(parent, &UNDERLINE_APPEARANCE);
        }
        if changes_choice(
            Some(parent.node()),
            &underline::LINE,
            &$format.underline_line,
            UnderlineLine::matches,
        ) {
            remove_choices(parent, &underline::LINE);
            if let Some(FormatChange::Set(line)) = &$format.underline_line {
                line.write(parent)?;
            }
        }
        if changes_choice(
            Some(parent.node()),
            &underline::FILL_CHOICE,
            &$format.underline_fill,
            UnderlineFill::matches,
        ) {
            remove_choices(parent, &underline::FILL_CHOICE);
            if let Some(FormatChange::Set(fill)) = &$format.underline_fill {
                fill.write(parent)?;
            }
        }
        if changes_choice(Some(parent.node()), &FILL, &$format.fill, fill_matches) {
            remove_choices(parent, &FILL);
            match &$format.fill {
                Some(FormatChange::Set(RunFill::Solid(color))) => {
                    color.write(&mut parent.insert::<dml::SolidFill>()?)?
                }
                Some(FormatChange::Set(RunFill::NoFill)) => {
                    parent.insert::<dml::NoFill>()?;
                }
                _ => {}
            }
        }
        if changes_choice(
            Some(parent.node()),
            &[dml::Highlight::QNAME],
            &$format.highlight,
            ColorSpec::matches,
        ) {
            parent.remove::<dml::Highlight>();
            if let Some(FormatChange::Set(color)) = &$format.highlight {
                color.write(&mut parent.insert::<dml::Highlight>()?)?;
            }
        }
        write_font!(parent, &$format.latin, LatinFont, LatinFontViewMut);
        write_font!(
            parent,
            &$format.east_asian,
            EastAsianFont,
            EastAsianFontViewMut
        );
        write_font!(
            parent,
            &$format.complex_script,
            ComplexScriptFont,
            ComplexScriptFontViewMut
        );
        write_font!(parent, &$format.symbol, SymbolFont, SymbolFontViewMut);
    };
}

macro_rules! run_attributes {
    ($($name:ident, $clear:ident: $ty:ty = $attr:ident, $parse:expr, $set:ident, $remove:ident, $arg:expr;)+) => {
        /// Formatting changes applied to the runs in a character range.
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct RunFormat {
            /// Underline line change.
            pub underline_line: Option<FormatChange<UnderlineLine>>,
            /// Underline fill change.
            pub underline_fill: Option<FormatChange<UnderlineFill>>,
            /// Text fill change.
            pub fill: Option<FormatChange<RunFill>>,
            /// Highlight colour change.
            pub highlight: Option<FormatChange<ColorSpec>>,
            /// Latin font change.
            pub latin: Option<FormatChange<FontRef>>,
            /// East Asian font change.
            pub east_asian: Option<FormatChange<FontRef>>,
            /// Complex-script font change.
            pub complex_script: Option<FormatChange<FontRef>>,
            /// Symbol font change.
            pub symbol: Option<FormatChange<FontRef>>,
            $(
                #[doc = concat!("Change to ", stringify!($name), ".")]
                pub $name: Option<FormatChange<$ty>>,
            )+
        }

        impl RunFormat {
            fn changes(&self, properties: Option<&RawNode>) -> bool {
                changes_choice(properties, &underline::LINE, &self.underline_line, UnderlineLine::matches)
                    || changes_choice(properties, &underline::FILL_CHOICE, &self.underline_fill, UnderlineFill::matches)
                    || changes_choice(properties, &FILL, &self.fill, fill_matches)
                    || changes_choice(properties, &[dml::Highlight::QNAME], &self.highlight, ColorSpec::matches)
                    || changes_choice(properties, &[dml::LatinFont::QNAME], &self.latin, font_matches)
                    || changes_choice(properties, &[dml::EastAsianFont::QNAME], &self.east_asian, font_matches)
                    || changes_choice(properties, &[dml::ComplexScriptFont::QNAME], &self.complex_script, font_matches)
                    || changes_choice(properties, &[dml::SymbolFont::QNAME], &self.symbol, font_matches)
                    || (matches!(self.underline, Some(FormatChange::Clear)) && properties.is_some_and(|node| node.elements().any(|child| UNDERLINE_APPEARANCE.iter().any(|&q| child.is(q)))))
                    $(|| self.$name.as_ref().is_some_and(|change| {
                    !run_attr_equals!(properties, RunProperties, $attr, change, $parse)
                }))+
            }
        }

        macro_rules! apply_attributes {
            ($format:expr, $view:ident, $element:ident) => {
                $(
                    if let Some(change) = &$format.$name
                        && !run_attr_equals!(Some($view.0.node()), $element, $attr, change, $parse)
                    {
                        match change {
                            FormatChange::Set(value) => $view.$set(($arg)(value))?,
                            FormatChange::Clear => { $view.$remove()?; }
                        }
                    }
                )+
            };
        }

        impl RunMut<'_> {
            $(
                #[doc = concat!("Sets the direct ", stringify!($name), ". Text size and spacing changes leave cached autofit geometry and scale in place.")]
                pub fn $name(&mut self, value: $ty) -> Result<()> {
                    self.format(&RunFormat { $name: Some(FormatChange::Set(value)), ..RunFormat::default() })
                }

                #[doc = concat!("Clears the direct ", stringify!($name), ".")]
                pub fn $clear(&mut self) -> Result<()> {
                    self.format(&RunFormat { $name: Some(FormatChange::Clear), ..RunFormat::default() })
                }
            )+
        }
    };
}

fn string(raw: &str) -> Option<String> {
    Some(raw.to_owned())
}

run_attributes! {
    size, clear_size: i32 = ATTR_SZ, int, set_font_size, remove_font_size, copy_arg;
    bold, clear_bold: bool = ATTR_B, parse_bool, set_bold, remove_bold, copy_arg;
    italic, clear_italic: bool = ATTR_I, parse_bool, set_italic, remove_italic, copy_arg;
    underline, clear_underline: TextUnderlineValues = ATTR_U, enum_value, set_underline, remove_underline, copy_arg;
    strike, clear_strike: TextStrikeValues = ATTR_STRIKE, enum_value, set_strike, remove_strike, copy_arg;
    baseline, clear_baseline: DmlPercent = ATTR_BASELINE, parse_dml_percent, set_baseline, remove_baseline, copy_arg;
    caps, clear_caps: TextCapsValues = ATTR_CAP, enum_value, set_capital, remove_capital, copy_arg;
    spacing, clear_spacing: i32 = ATTR_SPC, int, set_spacing, remove_spacing, copy_arg;
    kerning, clear_kerning: i32 = ATTR_KERN, int, set_kerning, remove_kerning, copy_arg;
    lang, clear_lang: String = ATTR_LANG, string, set_language, remove_language, str_arg;
    alt_lang, clear_alt_lang: String = ATTR_ALT_LANG, string, set_alternative_language, remove_alternative_language, str_arg;
}

impl RunFormat {
    fn validate(&self) -> Result<()> {
        if let Some(FormatChange::Set(line)) = &self.underline_line {
            line.validate()?;
        }
        if let Some(FormatChange::Set(UnderlineFill::Solid(color))) = &self.underline_fill {
            color.validate()?;
        }
        if matches!(self.underline, Some(FormatChange::Clear))
            && (matches!(self.underline_line, Some(FormatChange::Set(_)))
                || matches!(self.underline_fill, Some(FormatChange::Set(_))))
        {
            return Err(Error::ValueOutOfRange { what: "underline" });
        }
        if let Some(FormatChange::Set(RunFill::Solid(color))) = &self.fill {
            color.validate()?;
        }
        if let Some(FormatChange::Set(color)) = &self.highlight {
            color.validate()?;
        }
        for change in [
            &self.latin,
            &self.east_asian,
            &self.complex_script,
            &self.symbol,
        ] {
            if let Some(FormatChange::Set(font)) = change {
                check_attribute_text(font.typeface()?)?;
            }
        }
        for change in [&self.lang, &self.alt_lang] {
            if let Some(FormatChange::Set(value)) = change {
                check_attribute_text(value)?;
            }
        }
        for (change, min, max, attr) in [
            (&self.size, 100, 400_000, "sz"),
            (&self.spacing, -400_000, 400_000, "spc"),
            (&self.kerning, 0, 400_000, "kern"),
        ] {
            if let Some(FormatChange::Set(value)) = change {
                check_range(*value, min, max, attr)?;
            }
        }
        if let Some(FormatChange::Set(value)) = self.baseline
            && !(f64::from(i32::MIN)..=f64::from(i32::MAX))
                .contains(&(value.percent() * 1000.0).round())
        {
            return Err(Error::ValueOutOfRange { what: "baseline" });
        }
        Ok(())
    }

    fn apply(&self, run: &mut LensMut<'_, Run>) -> Result<()> {
        if self.changes(run.as_lens().child::<RunProperties>().map(|p| p.raw())) {
            let mut view = dml::RunPropertiesViewMut(run.get_or_insert::<RunProperties>()?);
            apply_attributes!(self, view, RunProperties);
            apply_children!(self, view.0);
        }
        Ok(())
    }
}

impl RunMut<'_> {
    /// Applies several direct run properties atomically.
    pub fn format(&mut self, format: &RunFormat) -> Result<()> {
        format.validate()?;
        para_shape(self.loc.node(&self.doc)?)?;
        let mut staged = self.doc.clone();
        let mut paragraph = self.loc.lens(&mut staged)?;
        let mut run = paragraph
            .nth_child_mut::<Run>(self.index)
            .ok_or(Error::RunNotFound(self.index))?;
        format.apply(&mut run)?;
        *self.doc = staged;
        Ok(())
    }
}

impl ParagraphMut<'_> {
    /// Applies formatting to the end-of-paragraph run properties.
    pub fn end_format(&mut self, format: &RunFormat) -> Result<()> {
        format.validate()?;
        para_shape(self.node()?)?;
        let mut staged = self.doc.clone();
        let mut paragraph = self.loc.lens(&mut staged)?;
        if format.changes(
            paragraph
                .as_lens()
                .child::<EndParagraphRunProperties>()
                .map(|p| p.raw()),
        ) {
            let mut view = dml::EndParagraphRunPropertiesViewMut(
                paragraph.get_or_insert::<EndParagraphRunProperties>()?,
            );
            apply_attributes!(format, view, EndParagraphRunProperties);
            apply_children!(format, view.0);
        }
        *self.doc = staged;
        Ok(())
    }

    fn update_properties(
        &mut self,
        changed: impl FnOnce(Option<&RawNode>) -> bool,
        update: impl FnOnce(dml::ParagraphPropertiesViewMut<'_>) -> Result<()>,
    ) -> Result<()> {
        let node = self.node()?;
        para_shape(node)?;
        if changed(
            node.elements()
                .find(|node| node.is(ParagraphProperties::QNAME)),
        ) {
            let mut staged = self.doc.clone();
            let mut paragraph = self.loc.lens(&mut staged)?;
            update(dml::ParagraphPropertiesViewMut(
                paragraph.get_or_insert::<ParagraphProperties>()?,
            ))?;
            *self.doc = staged;
        }
        Ok(())
    }
}

macro_rules! paragraph_attribute {
    ($method:ident, $clear:ident, $ty:ty, $attr:ident, $parse:expr, $set:ident, $remove:ident $(, $min:expr, $max:expr)?) => {
        impl ParagraphMut<'_> {
            #[doc = concat!("Sets the direct paragraph ", stringify!($method), ". PowerPoint may refit the text when reopened.")]
            pub fn $method(&mut self, value: $ty) -> Result<()> {
                $(check_range(value, $min, $max, ParagraphProperties::$attr.local)?;)?
                self.update_properties(
                    |node| !attr_equals(node, ParagraphProperties::$attr, &FormatChange::Set(value), $parse),
                    |mut view| Ok(view.$set(value)?),
                )
            }

            #[doc = concat!("Clears the direct paragraph ", stringify!($method), ".")]
            pub fn $clear(&mut self) -> Result<()> {
                self.update_properties(
                    |node| node.and_then(|node| node.attr(ParagraphProperties::$attr)).is_some(),
                    |mut view| { view.$remove()?; Ok(()) },
                )
            }
        }
    };
}

paragraph_attribute!(
    align,
    clear_align,
    TextAlignmentTypeValues,
    ATTR_ALGN,
    enum_value,
    set_alignment,
    remove_alignment
);
paragraph_attribute!(
    level,
    clear_level,
    i32,
    ATTR_LVL,
    int,
    set_level,
    remove_level,
    0,
    8
);
paragraph_attribute!(
    margin_left,
    clear_margin_left,
    i32,
    ATTR_MAR_L,
    int,
    set_left_margin,
    remove_left_margin,
    0,
    51_206_400
);
paragraph_attribute!(
    indent,
    clear_indent,
    i32,
    ATTR_INDENT,
    int,
    set_indent,
    remove_indent,
    -51_206_400,
    51_206_400
);
paragraph_attribute!(
    rtl,
    clear_rtl,
    bool,
    ATTR_RTL,
    parse_bool,
    set_right_to_left,
    remove_right_to_left
);
paragraph_attribute!(
    font_align,
    clear_font_align,
    TextFontAlignmentValues,
    ATTR_FONT_ALGN,
    enum_value,
    set_font_alignment,
    remove_font_alignment
);

fn copy_arg<T: Copy>(value: &T) -> T {
    *value
}

fn str_arg(value: &String) -> &str {
    value
}

impl RunMut<'_> {
    /// Writes the specified solid colour as the direct text fill.
    pub fn color(&mut self, color: ColorSpec) -> Result<()> {
        self.format(&RunFormat {
            fill: Some(FormatChange::Set(RunFill::Solid(color))),
            ..RunFormat::default()
        })
    }

    /// Suppresses the direct text fill.
    pub fn no_fill(&mut self) -> Result<()> {
        self.format(&RunFormat {
            fill: Some(FormatChange::Set(RunFill::NoFill)),
            ..RunFormat::default()
        })
    }

    /// Clears the direct text fill.
    pub fn clear_color(&mut self) -> Result<()> {
        self.format(&RunFormat {
            fill: Some(FormatChange::Clear),
            ..RunFormat::default()
        })
    }

    /// Writes the direct text highlight.
    pub fn highlight(&mut self, color: ColorSpec) -> Result<()> {
        self.format(&RunFormat {
            highlight: Some(FormatChange::Set(color)),
            ..RunFormat::default()
        })
    }

    /// Clears the direct text highlight.
    pub fn clear_highlight(&mut self) -> Result<()> {
        self.format(&RunFormat {
            highlight: Some(FormatChange::Clear),
            ..RunFormat::default()
        })
    }

    /// Sets a font slot. PowerPoint keeps cached autofit geometry and scale until refit.
    pub fn font(&mut self, font: FontRef, script: Script) -> Result<()> {
        self.update_font(script, FormatChange::Set(font))
    }

    /// Clears a font slot.
    pub fn clear_font(&mut self, script: Script) -> Result<()> {
        self.update_font(script, FormatChange::Clear)
    }

    fn update_font(&mut self, script: Script, change: FormatChange<FontRef>) -> Result<()> {
        let mut format = RunFormat::default();
        let slot = match script {
            Script::Latin => &mut format.latin,
            Script::EastAsian => &mut format.east_asian,
            Script::ComplexScript => &mut format.complex_script,
            Script::Symbol => &mut format.symbol,
        };
        *slot = Some(change);
        self.format(&format)
    }
}
