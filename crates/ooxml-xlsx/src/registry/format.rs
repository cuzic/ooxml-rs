//! The value-typed cell format (ADR-0006 §3).
//!
//! A [`Format`] is a set of optional groups. A group left `None` is
//! **inherited**: the new `xf` takes the workbook's default style's id for it
//! (`cellStyleXfs[0]`, else `cellXfs[0]`), so `Format::new()` looks like
//! the workbook's Normal style. A group that is set is written in full, in
//! one canonical XML form, and reused when an entry with exactly those bytes
//! already exists (ADR-0006 §2).
//!
//! Setting part of a group starts from that group's default: in edit mode
//! `set_bold()` starts from rust_xlsxwriter's Calibri 11, not from the
//! workbook's font. Start from [`Format::with_default_font_of`] to keep the
//! workbook's face and size.

/// A colour (ADR-0006 §3: raw values, no palette resolution).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// `rgb="AARRGGBB"`.
    Rgb(u32),
    /// `theme="index"` with an optional `tint` (the theme index is the
    /// attribute index: 0 = `lt1`, 1 = `dk1`, …).
    Theme {
        /// Theme colour index.
        index: u8,
        /// Tint in [-1, 1].
        tint: Tint,
    },
    /// `indexed="n"` (a legacy palette index; its meaning depends on the
    /// workbook's palette).
    Indexed(u8),
    /// `auto="1"`.
    Auto,
}

impl Color {
    /// An opaque RGB colour from `0xRRGGBB`.
    pub fn rgb(rrggbb: u32) -> Self {
        Color::Rgb(0xFF00_0000 | (rrggbb & 0x00FF_FFFF))
    }

    pub(crate) fn attrs(&self) -> Vec<(&'static str, String)> {
        match self {
            Color::Rgb(argb) => vec![("rgb", format!("{argb:08X}"))],
            Color::Theme { index, tint } if tint.0 == 0.0 => vec![("theme", index.to_string())],
            Color::Theme { index, tint } => vec![("theme", index.to_string()), ("tint", format!("{}", tint.0))],
            Color::Indexed(i) => vec![("indexed", i.to_string())],
            Color::Auto => vec![("auto", "1".into())],
        }
    }
}

/// A theme tint: finite, in [-1, 1], `-0.0` stored as `0.0`; equality and
/// hashing on the bits (ADR-0006 §3, review m9).
#[derive(Clone, Copy, Debug)]
pub struct Tint(f64);

impl Tint {
    /// `None` for NaN, infinities and values outside [-1, 1].
    pub fn new(t: f64) -> Option<Self> {
        (t.is_finite() && (-1.0..=1.0).contains(&t)).then_some(Tint(if t == 0.0 { 0.0 } else { t }))
    }
    /// No tint.
    pub const NONE: Tint = Tint(0.0);
    /// The value.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for Tint {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}
impl Eq for Tint {}
impl std::hash::Hash for Tint {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.to_bits().hash(h);
    }
}

/// A font size in points: finite, 1 to 409 (Excel's range), compared and
/// hashed on its bits; written in its shortest round-trip form (`11`,
/// `10.5`) (ADR-0006 §3, review m9).
#[derive(Clone, Copy, Debug)]
pub struct FontSize(f64);

impl FontSize {
    /// `None` for NaN, infinities and sizes outside 1..=409.
    pub fn new(points: f64) -> Option<Self> {
        (points.is_finite() && (1.0..=409.0).contains(&points)).then_some(FontSize(points))
    }
    /// The size in points.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for FontSize {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}
impl Eq for FontSize {}
impl std::hash::Hash for FontSize {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.to_bits().hash(h);
    }
}
impl std::fmt::Display for FontSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `f64`'s Display is the shortest form that round-trips
        write!(f, "{}", self.0)
    }
}

/// A number format.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NumFormat {
    /// Built-in id 0.
    General,
    /// A built-in id, 0–163 (ECMA-376 §18.8.30; refused if the workbook
    /// redefines it, and `InvalidFormat` from 164 up: those ids are custom,
    /// and an undefined one would later take the meaning of whatever custom
    /// format gets that id).
    Builtin(u16),
    /// Excel's default short date: built-in 14 (`mm-dd-yy`, shown in the
    /// user's short-date form), or the code `yyyy-mm-dd` when the workbook
    /// redefines id 14 (ADR-0006 §6). [`Format::date`] uses it.
    DefaultDate,
    /// A format code; a locale-independent built-in code maps to its id
    /// unless the workbook redefines that id.
    Code(String),
}

/// Underline styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Underline {
    /// `<u/>`.
    Single,
    /// `val="double"`.
    Double,
    /// `val="singleAccounting"`.
    SingleAccounting,
    /// `val="doubleAccounting"`.
    DoubleAccounting,
}

/// `vertAlign`: superscript, subscript or the explicit baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertAlign {
    /// `val="baseline"`.
    Baseline,
    /// `val="superscript"`.
    Superscript,
    /// `val="subscript"`.
    Subscript,
}

impl VertAlign {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            VertAlign::Baseline => "baseline",
            VertAlign::Superscript => "superscript",
            VertAlign::Subscript => "subscript",
        }
    }
}

/// A font (ADR-0006 §3; rust_xlsxwriter's feature set plus `charset`,
/// `condense` and `extend`, which East Asian default fonts carry).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Font {
    /// Face name.
    pub name: String,
    /// Size in points.
    pub size: FontSize,
    /// `<b/>`.
    pub bold: bool,
    /// `<i/>`.
    pub italic: bool,
    /// `<strike/>`.
    pub strike: bool,
    /// `<u/>`.
    pub underline: Option<Underline>,
    /// `<color/>`.
    pub color: Option<Color>,
    /// `<family val/>`.
    pub family: Option<u8>,
    /// `<scheme val/>` (`minor`, `major`).
    pub scheme: Option<String>,
    /// `<charset val/>` (128 = Shift-JIS, …; Japanese Excel's default font
    /// has one).
    pub charset: Option<u8>,
    /// `<vertAlign val/>`.
    pub vert_align: Option<VertAlign>,
    /// `<condense/>` (a Macintosh compatibility flag).
    pub condense: bool,
    /// `<extend/>` (a Macintosh compatibility flag).
    pub extend: bool,
}

impl Default for Font {
    /// rust_xlsxwriter's default: Calibri 11, theme colour 1, family 2,
    /// scheme minor.
    fn default() -> Self {
        Font {
            name: "Calibri".into(),
            size: FontSize(11.0),
            bold: false,
            italic: false,
            strike: false,
            underline: None,
            color: Some(Color::Theme { index: 1, tint: Tint::NONE }),
            family: Some(2),
            scheme: Some("minor".into()),
            charset: None,
            vert_align: None,
            condense: false,
            extend: false,
        }
    }
}

/// A fill.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Fill {
    /// `patternType="none"`.
    NoFill,
    /// A pattern (`solid`, `gray125`, `darkGray`, …) with optional
    /// foreground and background colours.
    Pattern {
        /// `patternType`.
        pattern: String,
        /// `fgColor`.
        fg: Option<Color>,
        /// `bgColor`.
        bg: Option<Color>,
    },
}

/// One border side: `style` (`thin`, `medium`, `dashed`, …) and colour.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Side {
    /// `style`.
    pub style: String,
    /// `color`.
    pub color: Option<Color>,
}

/// Borders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Border {
    /// Left.
    pub left: Option<Side>,
    /// Right.
    pub right: Option<Side>,
    /// Top.
    pub top: Option<Side>,
    /// Bottom.
    pub bottom: Option<Side>,
    /// Diagonal.
    pub diagonal: Option<Side>,
    /// `diagonalUp`.
    pub diagonal_up: bool,
    /// `diagonalDown`.
    pub diagonal_down: bool,
}

/// Alignment (attributes in schema order).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Alignment {
    /// `horizontal` (`left`, `center`, `right`, …).
    pub horizontal: Option<String>,
    /// `vertical` (`top`, `center`, `bottom`, …).
    pub vertical: Option<String>,
    /// `textRotation`.
    pub rotation: Option<u16>,
    /// `wrapText`.
    pub wrap: bool,
    /// `indent`.
    pub indent: Option<u8>,
    /// `shrinkToFit`.
    pub shrink: bool,
    /// `readingOrder`.
    pub reading_order: Option<u8>,
}

impl Alignment {
    pub(crate) fn attrs(&self) -> Vec<(&'static str, String)> {
        let mut v = Vec::new();
        if let Some(h) = &self.horizontal {
            v.push(("horizontal", h.clone()));
        }
        if let Some(x) = &self.vertical {
            v.push(("vertical", x.clone()));
        }
        if let Some(r) = self.rotation {
            v.push(("textRotation", r.to_string()));
        }
        if self.wrap {
            v.push(("wrapText", "1".into()));
        }
        if let Some(i) = self.indent {
            v.push(("indent", i.to_string()));
        }
        if self.shrink {
            v.push(("shrinkToFit", "1".into()));
        }
        if let Some(r) = self.reading_order {
            v.push(("readingOrder", r.to_string()));
        }
        v
    }
}

/// Protection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Protection {
    /// `locked` (Excel's default: true).
    pub locked: bool,
    /// `hidden`.
    pub hidden: bool,
}

impl Default for Protection {
    fn default() -> Self {
        Protection { locked: true, hidden: false }
    }
}

/// A cell format: every group optional (`None` = inherit from the default
/// style). See the module docs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Format {
    /// Number format.
    pub num_format: Option<NumFormat>,
    /// Font.
    pub font: Option<Font>,
    /// Fill.
    pub fill: Option<Fill>,
    /// Border.
    pub border: Option<Border>,
    /// Alignment.
    pub alignment: Option<Alignment>,
    /// Protection.
    pub protection: Option<Protection>,
    /// `quotePrefix`: the value is shown as text (Excel's leading
    /// apostrophe).
    pub quote_prefix: bool,
}

impl Format {
    /// Nothing set: looks like the workbook's default style.
    pub fn new() -> Self {
        Format::default()
    }

    /// A format whose font starts from `font` (e.g. the workbook's default
    /// font, [`crate::Workbook::default_font`]), so `set_bold` keeps the
    /// workbook's face and size.
    pub fn with_font(font: Font) -> Self {
        Format { font: Some(font), ..Format::default() }
    }

    fn font_mut(&mut self) -> &mut Font {
        self.font.get_or_insert_with(Font::default)
    }

    /// `"0.00"`, `"yyyy-mm-dd"`, …
    pub fn set_num_format(mut self, code: &str) -> Self {
        self.num_format = Some(NumFormat::Code(code.to_owned()));
        self
    }
    /// A built-in number format id.
    pub fn set_num_format_index(mut self, id: u16) -> Self {
        self.num_format = Some(NumFormat::Builtin(id));
        self
    }
    /// Bold.
    pub fn set_bold(mut self) -> Self {
        self.font_mut().bold = true;
        self
    }
    /// Italic.
    pub fn set_italic(mut self) -> Self {
        self.font_mut().italic = true;
        self
    }
    /// Strikethrough.
    pub fn set_strikethrough(mut self) -> Self {
        self.font_mut().strike = true;
        self
    }
    /// Underline.
    pub fn set_underline(mut self, u: Underline) -> Self {
        self.font_mut().underline = Some(u);
        self
    }
    /// Font size in points, written in its shortest form. A size
    /// [`FontSize::new`] rejects (not finite, or outside 1..=409) leaves the
    /// format unchanged; use [`FontSize::new`] directly to detect it.
    pub fn set_font_size(mut self, points: f64) -> Self {
        if let Some(size) = FontSize::new(points) {
            self.font_mut().size = size;
        }
        self
    }
    /// Font face.
    pub fn set_font_name(mut self, name: &str) -> Self {
        let f = self.font_mut();
        f.name = name.to_owned();
        // a face other than the theme's: no scheme, no family guess
        f.scheme = None;
        self
    }
    /// Superscript, subscript or baseline.
    pub fn set_font_script(mut self, v: VertAlign) -> Self {
        self.font_mut().vert_align = Some(v);
        self
    }
    /// Font character set (`charset`).
    pub fn set_font_charset(mut self, charset: u8) -> Self {
        self.font_mut().charset = Some(charset);
        self
    }
    /// Font colour.
    pub fn set_font_color(mut self, c: Color) -> Self {
        self.font_mut().color = Some(c);
        self
    }
    /// A solid fill.
    pub fn set_background_color(mut self, c: Color) -> Self {
        self.fill = Some(Fill::Pattern { pattern: "solid".into(), fg: Some(c), bg: None });
        self
    }
    /// The same border on all four sides (`thin`, `medium`, …).
    pub fn set_border(mut self, style: &str) -> Self {
        let side = Some(Side { style: style.to_owned(), color: None });
        self.border = Some(Border { left: side.clone(), right: side.clone(), top: side.clone(), bottom: side, ..Border::default() });
        self
    }
    /// Horizontal alignment.
    pub fn set_align(mut self, horizontal: &str) -> Self {
        self.alignment.get_or_insert_with(Alignment::default).horizontal = Some(horizontal.to_owned());
        self
    }
    /// Vertical alignment.
    pub fn set_valign(mut self, vertical: &str) -> Self {
        self.alignment.get_or_insert_with(Alignment::default).vertical = Some(vertical.to_owned());
        self
    }
    /// Wrap text.
    pub fn set_text_wrap(mut self) -> Self {
        self.alignment.get_or_insert_with(Alignment::default).wrap = true;
        self
    }
    /// Unlocked (for protected sheets).
    pub fn set_unlocked(mut self) -> Self {
        self.protection.get_or_insert_with(Protection::default).locked = false;
        self
    }
    /// Hidden formula (for protected sheets).
    pub fn set_hidden(mut self) -> Self {
        self.protection.get_or_insert_with(Protection::default).hidden = true;
        self
    }
    /// `quotePrefix="1"` (Excel's leading apostrophe: the value is text).
    pub fn set_quote_prefix(mut self) -> Self {
        self.quote_prefix = true;
        self
    }
    /// The default date format ([`NumFormat::DefaultDate`]: built-in 14,
    /// shown in the user's short-date form, or `yyyy-mm-dd` when the
    /// workbook redefines 14).
    pub fn date() -> Self {
        Format { num_format: Some(NumFormat::DefaultDate), ..Format::default() }
    }
}

impl Format {
    /// Checks the string-typed values against their schema enumerations
    /// (`ST_PatternType`, `ST_BorderStyle`, `ST_HorizontalAlignment`,
    /// `ST_VerticalAlignment`, `ST_FontScheme`) and the numeric ranges
    /// (`textRotation` 0–180 or 255, `readingOrder` 0–2), so a write never
    /// produces a `styles.xml` Excel rejects. [`crate::Error::InvalidFormat`].
    pub(crate) fn validate(&self) -> crate::Result<()> {
        use ooxml_core::EnumType;
        use ooxml_schema::sml::{BorderStyleValues, FontSchemeValues, HorizontalAlignmentValues, PatternValues, VerticalAlignmentValues};
        fn one_of<E: EnumType>(what: &str, v: &str) -> crate::Result<()> {
            if E::VALUES.contains(&v) { Ok(()) } else { Err(crate::Error::InvalidFormat(format!("{what} {v:?}"))) }
        }
        let bad = |what: String| Err(crate::Error::InvalidFormat(what));
        // Excel's palette has 12 theme colours (0 = lt1 … 11 = folHlink).
        let color = |c: &Option<Color>| match c {
            Some(Color::Theme { index, .. }) if *index > 11 => bad(format!("theme colour index {index} (0-11)")),
            _ => Ok(()),
        };
        if let Some(f) = &self.font {
            if f.name.is_empty() {
                return bad("empty font name".into());
            }
            // Excel's limit for a font name (review m4)
            if f.name.chars().count() > 31 {
                return bad(format!("font name {:?} is longer than 31 characters", f.name));
            }
            if let Some(fam) = f.family
                && fam > 14
            {
                return bad(format!("font family {fam} (ST_FontFamily: 0-14)"));
            }
            if let Some(s) = &f.scheme {
                one_of::<FontSchemeValues>("font scheme", s)?;
            }
            color(&f.color)?;
        }
        if let Some(Fill::Pattern { pattern, fg, bg }) = &self.fill {
            one_of::<PatternValues>("fill pattern", pattern)?;
            color(fg)?;
            color(bg)?;
        }
        if let Some(b) = &self.border {
            for s in [&b.left, &b.right, &b.top, &b.bottom, &b.diagonal].into_iter().flatten() {
                one_of::<BorderStyleValues>("border style", &s.style)?;
                color(&s.color)?;
            }
        }
        if let Some(NumFormat::Code(c)) = &self.num_format
            && c.is_empty()
        {
            return bad("empty number format code".into());
        }
        if let Some(a) = &self.alignment {
            if let Some(h) = &a.horizontal {
                one_of::<HorizontalAlignmentValues>("horizontal alignment", h)?;
            }
            if let Some(v) = &a.vertical {
                one_of::<VerticalAlignmentValues>("vertical alignment", v)?;
            }
            if a.rotation.is_some_and(|r| r > 180 && r != 255) {
                return Err(crate::Error::InvalidFormat(format!("text rotation {}", a.rotation.unwrap_or_default())));
            }
            if a.indent.is_some_and(|i| i > 250) {
                return Err(crate::Error::InvalidFormat(format!("indent {} (Excel's maximum is 250)", a.indent.unwrap_or_default())));
            }
            if a.reading_order.is_some_and(|r| r > 2) {
                return Err(crate::Error::InvalidFormat(format!("reading order {}", a.reading_order.unwrap_or_default())));
            }
        }
        Ok(())
    }
}

/// The font part of a differential format (`dxf/font`). In a `dxf` an
/// omitted property means "do not override" (ADR-0006 §3, §5.8), so every
/// property is optional and only the set ones are written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DxfFont {
    /// `<b/>`.
    pub bold: bool,
    /// `<i/>`.
    pub italic: bool,
    /// `<strike/>`.
    pub strike: bool,
    /// `<u/>`.
    pub underline: Option<Underline>,
    /// `<color/>`.
    pub color: Option<Color>,
    /// `<sz/>`.
    pub size: Option<FontSize>,
    /// `<name/>`.
    pub name: Option<String>,
}

/// A differential format (`dxf`), as conditional formats and table styles
/// use (ADR-0006 §5.8; the API that applies them is Phase4). Every group is
/// an override: `None` leaves that part of the cell's look alone. There is
/// no "inherit" and no default-style id is copied.
///
/// A solid fill in a `dxf` takes its colour from `bgColor` (Excel's form,
/// written without `patternType`), which [`DxfFormat::set_background_color`]
/// produces.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DxfFormat {
    /// Number format.
    pub num_format: Option<NumFormat>,
    /// Font overrides.
    pub font: Option<DxfFont>,
    /// Fill.
    pub fill: Option<Fill>,
    /// Borders (only the sides that are set are written).
    pub border: Option<Border>,
    /// Alignment.
    pub alignment: Option<Alignment>,
    /// Protection.
    pub protection: Option<Protection>,
}

impl DxfFormat {
    /// Overrides nothing.
    pub fn new() -> Self {
        DxfFormat::default()
    }
    fn font_mut(&mut self) -> &mut DxfFont {
        self.font.get_or_insert_with(DxfFont::default)
    }
    /// Bold.
    pub fn set_bold(mut self) -> Self {
        self.font_mut().bold = true;
        self
    }
    /// Italic.
    pub fn set_italic(mut self) -> Self {
        self.font_mut().italic = true;
        self
    }
    /// Font colour.
    pub fn set_font_color(mut self, c: Color) -> Self {
        self.font_mut().color = Some(c);
        self
    }
    /// A solid fill of this colour (`bgColor`, as Excel writes a dxf).
    pub fn set_background_color(mut self, c: Color) -> Self {
        self.fill = Some(Fill::Pattern { pattern: "solid".into(), fg: None, bg: Some(c) });
        self
    }
    /// A number format code.
    pub fn set_num_format(mut self, code: &str) -> Self {
        self.num_format = Some(NumFormat::Code(code.to_owned()));
        self
    }
    /// The same border on all four sides.
    pub fn set_border(mut self, style: &str) -> Self {
        let side = Some(Side { style: style.to_owned(), color: None });
        self.border = Some(Border { left: side.clone(), right: side.clone(), top: side.clone(), bottom: side, ..Border::default() });
        self
    }

    /// The same checks as [`Format`]'s, on the parts a `dxf` has.
    pub(crate) fn validate(&self) -> crate::Result<()> {
        let as_format = Format {
            num_format: self.num_format.clone(),
            font: None,
            fill: self.fill.clone(),
            border: self.border.clone(),
            alignment: self.alignment.clone(),
            protection: self.protection,
            quote_prefix: false,
        };
        as_format.validate()?;
        if let Some(f) = &self.font {
            // the font checks of `Format::validate`, on a stand-in font
            let probe = Font { name: f.name.clone().unwrap_or_else(|| "x".into()), color: f.color, ..Font::default() };
            Format::with_font(probe).validate()?;
        }
        Ok(())
    }
}

/// Built-in ids whose display depends on the user's locale although their
/// table code is fixed: a code is never mapped to them (PR #50 review m1),
/// only an explicit `Builtin` or [`NumFormat::DefaultDate`] uses them.
pub(crate) const LOCALE_DEPENDENT: &[u16] = &[14, 22];

/// ECMA-376 Part 1 §18.8.30 built-in number formats.
pub(crate) const BUILTIN: &[(u16, &str)] = &[
    (0, "General"),
    (1, "0"),
    (2, "0.00"),
    (3, "#,##0"),
    (4, "#,##0.00"),
    (9, "0%"),
    (10, "0.00%"),
    (11, "0.00E+00"),
    (12, "# ?/?"),
    (13, "# ??/??"),
    (14, "mm-dd-yy"),
    (15, "d-mmm-yy"),
    (16, "d-mmm"),
    (17, "mmm-yy"),
    (18, "h:mm AM/PM"),
    (19, "h:mm:ss AM/PM"),
    (20, "h:mm"),
    (21, "h:mm:ss"),
    (22, "m/d/yy h:mm"),
    (37, "#,##0 ;(#,##0)"),
    (38, "#,##0 ;[Red](#,##0)"),
    (39, "#,##0.00;(#,##0.00)"),
    (40, "#,##0.00;[Red](#,##0.00)"),
    (45, "mm:ss"),
    (46, "[h]:mm:ss"),
    (47, "mmss.0"),
    (48, "##0.0E+0"),
    (49, "@"),
];

/// Is this a date or time number format? calamine's rule
/// (`src/formats.rs`): built-in ids 14–22, 27–36, 45, 47, 50–58 (46 is a
/// duration), or a custom code with date/time tokens outside quoted text
/// and `[...]` sections (an elapsed `[h]` counts).
pub(crate) fn is_date_format(builtin_id: Option<u32>, code: Option<&str>) -> bool {
    if let Some(code) = code {
        return code_is_date(code);
    }
    matches!(builtin_id, Some(14..=22 | 27..=36 | 45 | 47 | 50..=58))
}

fn code_is_date(code: &str) -> bool {
    // only the first section counts
    let mut in_quote = false;
    let mut in_bracket = false;
    let mut escaped = false;
    let mut bracket = String::new();
    for c in code.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' | '_' | '*' if !in_quote => escaped = true,
            '"' => in_quote = !in_quote,
            '[' if !in_quote => {
                in_bracket = true;
                bracket.clear();
            }
            ']' if in_bracket => {
                in_bracket = false;
                let b = bracket.to_ascii_lowercase();
                if b.chars().all(|c| matches!(c, 'h' | 'm' | 's')) && !b.is_empty() {
                    return true;
                }
            }
            ';' if !in_quote && !in_bracket => break,
            c if in_bracket => bracket.push(c),
            c if !in_quote && matches!(c.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's') => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formats() {
        for d in ["yyyy-mm-dd", "d-mmm", "h:mm", "[h]:mm:ss", "mm/dd/yy;@", "[$-409]d-mmm-yy"] {
            assert!(code_is_date(d), "{d}");
        }
        for n in ["0.00", "#,##0", "\"day\" 0", "[Red]0.00", "@", "General", "0.000\" proto\""] {
            assert!(!code_is_date(n), "{n}");
        }
        assert!(is_date_format(Some(14), None));
        assert!(!is_date_format(Some(46), None));
        assert!(!is_date_format(Some(2), None));
    }

    #[test]
    fn tints_and_colors() {
        assert_eq!(Tint::new(-0.0), Some(Tint::NONE));
        assert!(Tint::new(f64::NAN).is_none() && Tint::new(1.5).is_none());
        assert_eq!(Color::rgb(0xFF0000).attrs(), vec![("rgb", "FFFF0000".to_string())]);
        let t = Color::Theme { index: 4, tint: Tint::new(-0.249977111117893).unwrap() };
        assert_eq!(t.attrs()[1].1, "-0.249977111117893");
    }
}
