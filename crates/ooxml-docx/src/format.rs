//! Direct formatting (ADR-0007 §4) and style references (§5's setters).
//!
//! Every setter plans its writes over the read model, dry-runs every
//! insertion against the unchanged tree (`check_insert_at`, which for these
//! bounded slots places against the whole parent, so producers' out-of-order
//! `w:rPr`/`w:pPr` refuse), checks that every new `w:` attribute will find
//! a prefixed WML binding in scope (review n4), and only then writes.

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ChildOf, Content, ElementSpec, LensMut, QName, RawNode};
use ooxml_schema::wml::{
    Bold, BoldComplexScript, Color as ColorEl, FontSize, FontSizeComplexScript, Highlight as HighlightEl,
    Indentation, Italic, ItalicComplexScript, Justification, KeepLines, KeepNext, NS_W, Paragraph as P,
    ParagraphProperties, ParagraphStyleId, Run as R, RunFonts, RunProperties, RunStyle, SpacingBetweenLines, Strike,
    Underline as UnderlineEl, VerticalTextAlignment,
};

use crate::RunContext;
use crate::body::Wrapper;
use crate::document::{Document, node_at};
use crate::error::{Error, Result};
use crate::fields::Operation;
use crate::names::{RPR_CHANGE, is};
use crate::story::{ParaRef, RunRef};

/// A property value to write: keep what is there, set it, or remove the
/// element so that the value is inherited again (ADR-0007 §4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Prop<T> {
    /// Leave it as it is (the default).
    #[default]
    Keep,
    /// Write this value.
    Set(T),
    /// Remove the direct value.
    Clear,
}

/// `w:color/@w:val`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// `auto`.
    Auto,
    /// `RRGGBB`.
    Rgb(u8, u8, u8),
}

impl Color {
    fn lexical(self) -> String {
        match self {
            Color::Auto => "auto".into(),
            Color::Rgb(r, g, b) => format!("{r:02X}{g:02X}{b:02X}"),
        }
    }
}

/// A font size in half-points, 1..=3276 (Word's 1638 pt limit *(u)*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HalfPoints(u16);

impl HalfPoints {
    /// `None` outside 1..=3276.
    pub fn new(half_points: u16) -> Option<Self> {
        (1..=3276).contains(&half_points).then_some(HalfPoints(half_points))
    }

    /// The value.
    pub fn get(self) -> u16 {
        self.0
    }
}

macro_rules! lexical_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name { $($(#[$vm])* $v),* }
        impl $name {
            /// The value as written in the file.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$v => $s),* }
            }
        }
    };
}

lexical_enum!(
    /// `ST_Underline`.
    Underline {
        /// `single`
        Single = "single", /// `words`
        Words = "words", /// `double`
        Double = "double", /// `thick`
        Thick = "thick", /// `dotted`
        Dotted = "dotted", /// `dottedHeavy`
        DottedHeavy = "dottedHeavy", /// `dash`
        Dash = "dash", /// `dashedHeavy`
        DashedHeavy = "dashedHeavy", /// `dashLong`
        DashLong = "dashLong", /// `dashLongHeavy`
        DashLongHeavy = "dashLongHeavy", /// `dotDash`
        DotDash = "dotDash", /// `dashDotHeavy`
        DashDotHeavy = "dashDotHeavy", /// `dotDotDash`
        DotDotDash = "dotDotDash", /// `dashDotDotHeavy`
        DashDotDotHeavy = "dashDotDotHeavy", /// `wave`
        Wave = "wave", /// `wavyHeavy`
        WavyHeavy = "wavyHeavy", /// `wavyDouble`
        WavyDouble = "wavyDouble", /// `none`
        None = "none",
    }
);

lexical_enum!(
    /// `ST_HighlightColor` (17 values).
    Highlight {
        /// `black`
        Black = "black", /// `blue`
        Blue = "blue", /// `cyan`
        Cyan = "cyan", /// `green`
        Green = "green", /// `magenta`
        Magenta = "magenta", /// `red`
        Red = "red", /// `yellow`
        Yellow = "yellow", /// `white`
        White = "white", /// `darkBlue`
        DarkBlue = "darkBlue", /// `darkCyan`
        DarkCyan = "darkCyan", /// `darkGreen`
        DarkGreen = "darkGreen", /// `darkMagenta`
        DarkMagenta = "darkMagenta", /// `darkRed`
        DarkRed = "darkRed", /// `darkYellow`
        DarkYellow = "darkYellow", /// `darkGray`
        DarkGray = "darkGray", /// `lightGray`
        LightGray = "lightGray", /// `none`
        None = "none",
    }
);

lexical_enum!(
    /// `w:vertAlign` values.
    VertAlign {
        /// `baseline`
        Baseline = "baseline", /// `superscript`
        Superscript = "superscript", /// `subscript`
        Subscript = "subscript",
    }
);

lexical_enum!(
    /// `w:jc` values Phase1 writes: `left`/`right`, not `start`/`end` (ADR-0007 §4, Q9).
    Alignment {
        /// `left`
        Left = "left", /// `center`
        Center = "center", /// `right`
        Right = "right", /// `both` (justified)
        Both = "both", /// `distribute`
        Distribute = "distribute",
    }
);

/// The four font slots of `w:rFonts`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontSlots<T> {
    /// `w:ascii`
    pub ascii: T,
    /// `w:hAnsi`
    pub h_ansi: T,
    /// `w:eastAsia`
    pub east_asia: T,
    /// `w:cs`
    pub cs: T,
}

/// Line spacing (`w:spacing/@w:line` + `@w:lineRule`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineSpacing {
    /// `auto`, in 240ths of a line (240 = single).
    Auto(u32),
    /// `exact`, in twips.
    Exact(u32),
    /// `atLeast`, in twips.
    AtLeast(u32),
}

/// First-line indentation (`w:ind/@w:firstLine` or `@w:hanging`), twips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FirstLine {
    /// `w:firstLine`
    Indent(u32),
    /// `w:hanging`
    Hanging(u32),
}

/// Run properties to write (ADR-0007 §4). `Default` keeps everything; build
/// one with struct update syntax (`RunFormat { bold: Prop::Set(true),
/// ..RunFormat::default() }`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunFormat {
    /// `w:b`
    pub bold: Prop<bool>,
    /// `w:bCs`
    pub bold_cs: Prop<bool>,
    /// `w:i`
    pub italic: Prop<bool>,
    /// `w:iCs`
    pub italic_cs: Prop<bool>,
    /// `w:strike`
    pub strike: Prop<bool>,
    /// `w:u/@w:val` (the underline colour attributes are kept)
    pub underline: Prop<Underline>,
    /// `w:color/@w:val`; setting it removes `w:themeColor`, `w:themeTint`, `w:themeShade`
    pub color: Prop<Color>,
    /// `w:sz`
    pub size: Prop<HalfPoints>,
    /// `w:szCs`
    pub size_cs: Prop<HalfPoints>,
    /// `w:highlight`
    pub highlight: Prop<Highlight>,
    /// `w:vertAlign`
    pub vert_align: Prop<VertAlign>,
    /// `w:rFonts` slots; setting one removes that slot's theme attribute (`w:asciiTheme`, …, `w:cstheme`)
    pub fonts: FontSlots<Prop<String>>,
}

/// Paragraph properties to write (ADR-0007 §4). `Default` keeps everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParagraphFormat {
    /// `w:jc`
    pub alignment: Prop<Alignment>,
    /// `w:spacing/@w:before`, twips
    pub space_before: Prop<u32>,
    /// `w:spacing/@w:after`, twips
    pub space_after: Prop<u32>,
    /// `w:spacing/@w:line` + `@w:lineRule`
    pub line_spacing: Prop<LineSpacing>,
    /// `w:ind/@w:left`, twips
    pub indent_left: Prop<i32>,
    /// `w:ind/@w:right`, twips
    pub indent_right: Prop<i32>,
    /// `w:ind/@w:firstLine` or `@w:hanging`
    pub first_line: Prop<FirstLine>,
    /// `w:keepNext`
    pub keep_next: Prop<bool>,
    /// `w:keepLines`
    pub keep_lines: Prop<bool>,
}

// ---------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------

/// Which property element (dispatches to its generated type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    RStyle,
    RFonts,
    B,
    BCs,
    I,
    ICs,
    Strike,
    Color,
    Sz,
    SzCs,
    Highlight,
    U,
    VertAlign,
    PStyle,
    KeepNext,
    KeepLines,
    Spacing,
    Ind,
    Jc,
}

impl Kind {
    fn qname(self) -> QName {
        match self {
            Kind::RStyle => RunStyle::QNAME,
            Kind::RFonts => RunFonts::QNAME,
            Kind::B => Bold::QNAME,
            Kind::BCs => BoldComplexScript::QNAME,
            Kind::I => Italic::QNAME,
            Kind::ICs => ItalicComplexScript::QNAME,
            Kind::Strike => Strike::QNAME,
            Kind::Color => ColorEl::QNAME,
            Kind::Sz => FontSize::QNAME,
            Kind::SzCs => FontSizeComplexScript::QNAME,
            Kind::Highlight => HighlightEl::QNAME,
            Kind::U => UnderlineEl::QNAME,
            Kind::VertAlign => VerticalTextAlignment::QNAME,
            Kind::PStyle => ParagraphStyleId::QNAME,
            Kind::KeepNext => KeepNext::QNAME,
            Kind::KeepLines => KeepLines::QNAME,
            Kind::Spacing => SpacingBetweenLines::QNAME,
            Kind::Ind => Indentation::QNAME,
            Kind::Jc => Justification::QNAME,
        }
    }
}

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}
const VAL: QName = w("val");

/// The planned change of one property element.
#[derive(Clone, Debug)]
pub(crate) struct PropPlan {
    pub kind: Kind,
    /// Item index of the existing element in its container.
    pub existing: Option<usize>,
    pub remove_element: bool,
    pub set: Vec<(QName, String)>,
    pub remove: Vec<QName>,
}

/// Planned changes of one container (`w:rPr` of a run, `w:pPr` of a paragraph).
#[derive(Clone, Debug, Default)]
pub(crate) struct ContainerPlan {
    pub props: Vec<PropPlan>,
}

impl ContainerPlan {
    /// Does the plan only remove (elements or attributes)? Only then can it
    /// leave its container empty, and the empty container be removed.
    pub(crate) fn removes_only(&self) -> bool {
        self.props.iter().all(|p| p.remove_element || (p.set.is_empty() && !p.remove.is_empty()))
    }
}

/// No element child, no attribute other than namespace declarations, no
/// text other than whitespace.
fn is_empty_container(n: &RawNode) -> bool {
    n.elements().next().is_none()
        && n.attrs().iter().all(|a| a.name().is_namespace_declaration())
        && n.children().iter().all(|c| !matches!(c, ooxml_core::RawItem::Text(t) if !t.is_whitespace()))
}

/// Builds `PropPlan`s against an existing container (or none).
pub(crate) struct Planner<'a> {
    container: Option<&'a RawNode>,
    pub plan: ContainerPlan,
}

fn reads_on(e: &RawNode) -> Option<bool> {
    match e.attr(VAL) {
        None => Some(true),
        Some(v) => ooxml_core::value::parse_on_off(v),
    }
}

impl<'a> Planner<'a> {
    pub(crate) fn new(container: Option<&'a RawNode>) -> Self {
        Planner { container, plan: ContainerPlan::default() }
    }

    /// The single existing element of `kind`, if its siblings can be resolved.
    fn existing(&self, kind: Kind) -> Result<Option<(usize, &'a RawNode)>> {
        let Some(c) = self.container else { return Ok(None) };
        if c.elements().any(|e| e.is(MC_ALTERNATE_CONTENT)) {
            let parent = if c.is(RunProperties::QNAME) { RunProperties::QNAME } else { ParagraphProperties::QNAME };
            return Err(Error::Core(ooxml_core::Error::UnresolvedSibling(parent)));
        }
        let q = kind.qname();
        let mut found = c.indexed_elements().filter(|(_, e)| e.is(q));
        let first = found.next();
        if found.next().is_some() {
            return Err(Error::DuplicateProperty { name: format!("w:{}", q.local) });
        }
        Ok(first)
    }

    fn push(&mut self, kind: Kind, existing: Option<(usize, &RawNode)>, set: Vec<(QName, String)>, remove: Vec<QName>) {
        // drop no-op sets and removals of absent attributes
        let set: Vec<(QName, String)> = set
            .into_iter()
            .filter(|(q, v)| existing.is_none_or(|(_, e)| e.attr(*q) != Some(v.as_str())))
            .collect();
        let remove: Vec<QName> =
            remove.into_iter().filter(|q| existing.is_some_and(|(_, e)| e.attr(*q).is_some())).collect();
        if set.is_empty() && remove.is_empty() {
            return;
        }
        self.plan.props.push(PropPlan { kind, existing: existing.map(|(i, _)| i), remove_element: false, set, remove });
    }

    fn clear(&mut self, kind: Kind) -> Result<()> {
        if let Some((i, _)) = self.existing(kind)? {
            self.plan.props.push(PropPlan { kind, existing: Some(i), remove_element: true, set: vec![], remove: vec![] });
        }
        Ok(())
    }

    pub(crate) fn on_off(&mut self, kind: Kind, prop: &Prop<bool>) -> Result<()> {
        match prop {
            Prop::Keep => Ok(()),
            Prop::Clear => self.clear(kind),
            Prop::Set(v) => {
                let ex = self.existing(kind)?;
                if ex.is_some_and(|(_, e)| reads_on(e) == Some(*v)) {
                    return Ok(());
                }
                let (set, remove) = if *v { (vec![], vec![VAL]) } else { (vec![(VAL, "0".to_owned())], vec![]) };
                match ex {
                    // `<w:b/>` for true: a new element needs no attribute
                    None if *v => {
                        self.plan.props.push(PropPlan { kind, existing: None, remove_element: false, set, remove: vec![] });
                    }
                    _ => self.push(kind, ex, set, remove),
                }
                Ok(())
            }
        }
    }

    /// An element whose only written attribute is `w:val`.
    pub(crate) fn val(&mut self, kind: Kind, prop: Prop<String>, remove: &[&'static str]) -> Result<()> {
        match prop {
            Prop::Keep => Ok(()),
            Prop::Clear => self.clear(kind),
            Prop::Set(v) => {
                let ex = self.existing(kind)?;
                self.push(kind, ex, vec![(VAL, v)], remove.iter().map(|l| w(l)).collect());
                Ok(())
            }
        }
    }

    /// Attribute-level writes into one element (`w:rFonts`, `w:spacing`,
    /// `w:ind`). An element left with no attribute and no child by removals
    /// is removed.
    pub(crate) fn attrs(&mut self, kind: Kind, set: Vec<(QName, String)>, remove: Vec<QName>) -> Result<()> {
        if set.is_empty() && remove.is_empty() {
            return Ok(());
        }
        let ex = self.existing(kind)?;
        if set.is_empty()
            && let Some((i, e)) = ex
        {
            let left = e.attrs().iter().filter(|a| !a.name().is_namespace_declaration()).filter(|a| !remove.iter().any(|q| a.name().is(*q))).count();
            if left == 0 && e.elements().next().is_none() && remove.iter().any(|q| e.attr(*q).is_some()) {
                self.plan.props.push(PropPlan { kind, existing: Some(i), remove_element: true, set: vec![], remove: vec![] });
                return Ok(());
            }
        }
        self.push(kind, ex, set, remove);
        Ok(())
    }
}

fn lexical<T: Copy>(p: &Prop<T>, s: impl Fn(T) -> &'static str) -> Prop<String> {
    match p {
        Prop::Keep => Prop::Keep,
        Prop::Clear => Prop::Clear,
        Prop::Set(v) => Prop::Set(s(*v).to_owned()),
    }
}

/// Plans a [`RunFormat`] against a run's `w:rPr` (or none).
pub(crate) fn plan_run(rpr: Option<&RawNode>, f: &RunFormat) -> Result<ContainerPlan> {
    let mut pl = Planner::new(rpr);
    pl.on_off(Kind::B, &f.bold)?;
    pl.on_off(Kind::BCs, &f.bold_cs)?;
    pl.on_off(Kind::I, &f.italic)?;
    pl.on_off(Kind::ICs, &f.italic_cs)?;
    pl.on_off(Kind::Strike, &f.strike)?;
    pl.val(Kind::U, lexical(&f.underline, |u| u.as_str()), &[])?;
    // colour: an equal value is a no-op only when no theme attribute supersedes it
    if let Prop::Set(c) = f.color {
        let ex = pl.existing(Kind::Color)?;
        let want = c.lexical();
        let same = ex.is_some_and(|(_, e)| e.attr(VAL).is_some_and(|v| v.eq_ignore_ascii_case(&want)));
        let theme = ["themeColor", "themeTint", "themeShade"];
        if !same || ex.is_some_and(|(_, e)| theme.iter().any(|t| e.attr(w(t)).is_some())) {
            let set = if same { vec![] } else { vec![(VAL, want)] };
            pl.push(Kind::Color, ex, set, theme.iter().map(|t| w(t)).collect());
        }
    } else if f.color == Prop::Clear {
        pl.clear(Kind::Color)?;
    }
    for (kind, p) in [(Kind::Sz, &f.size), (Kind::SzCs, &f.size_cs)] {
        let v = match p {
            Prop::Set(h) => {
                let ex = pl.existing(kind)?;
                // numeric comparison: `24` and `24.0` are not both integers, so raw equality is used after trimming
                if ex.is_some_and(|(_, e)| e.attr(VAL).map(str::trim) == Some(h.get().to_string().as_str())) {
                    continue;
                }
                Prop::Set(h.get().to_string())
            }
            Prop::Clear => Prop::Clear,
            Prop::Keep => Prop::Keep,
        };
        pl.val(kind, v, &[])?;
    }
    pl.val(Kind::Highlight, lexical(&f.highlight, |h| h.as_str()), &[])?;
    pl.val(Kind::VertAlign, lexical(&f.vert_align, |v| v.as_str()), &[])?;
    // fonts: one element, attribute-level
    let mut set = Vec::new();
    let mut remove = Vec::new();
    for (p, slot, theme) in [
        (&f.fonts.ascii, "ascii", "asciiTheme"),
        (&f.fonts.h_ansi, "hAnsi", "hAnsiTheme"),
        (&f.fonts.east_asia, "eastAsia", "eastAsiaTheme"),
        (&f.fonts.cs, "cs", "cstheme"),
    ] {
        match p {
            Prop::Keep => {}
            Prop::Set(name) => {
                if name.is_empty() {
                    return Err(Error::InvalidValue { what: "font name", value: String::new() });
                }
                // at plan time, so that nothing is written (review of PR #59, M2)
                if let Some((_, ch)) = ooxml_core::value::first_invalid_xml_char(name) {
                    return Err(Error::InvalidXmlChar { ch });
                }
                set.push((w(slot), name.clone()));
                remove.push(w(theme));
            }
            Prop::Clear => {
                remove.push(w(slot));
                remove.push(w(theme));
            }
        }
    }
    pl.attrs(Kind::RFonts, set, remove)?;
    Ok(pl.plan)
}

/// Inherited superseding attributes found on a paragraph's style chain
/// (ADR-0007 §4, review m3): `(element local name, attribute local name)` →
/// the resolved value, most specific level first.
pub(crate) type Inherited<'a> = &'a dyn Fn(&'static str, &'static str) -> Result<Option<String>>;

fn active(v: &Option<String>, autospacing: bool) -> bool {
    match v {
        None => false,
        Some(v) if autospacing => ooxml_core::value::parse_on_off(v) != Some(false),
        Some(v) => v.trim().parse::<i64>().ok().is_none_or(|n| n != 0),
    }
}

/// Plans a [`ParagraphFormat`] against a paragraph's `w:pPr` (or none).
pub(crate) fn plan_paragraph(ppr: Option<&RawNode>, f: &ParagraphFormat, inherited: Inherited<'_>) -> Result<ContainerPlan> {
    let mut pl = Planner::new(ppr);
    pl.on_off(Kind::KeepNext, &f.keep_next)?;
    pl.on_off(Kind::KeepLines, &f.keep_lines)?;
    // spacing
    let mut set = Vec::new();
    let mut remove = Vec::new();
    // `superseding`: (attribute, is an on/off autospacing flag)
    let supersede = |set: &mut Vec<(QName, String)>, remove: &mut Vec<QName>, el: &'static str, attrs: &[(&'static str, bool)]| -> Result<()> {
        for &(a, flag) in attrs {
            if active(&inherited(el, a)?, flag) {
                set.push((w(a), "0".into()));
            } else {
                remove.push(w(a));
            }
        }
        Ok(())
    };
    for (p, attr, sup) in [
        (&f.space_before, "before", [("beforeLines", false), ("beforeAutospacing", true)]),
        (&f.space_after, "after", [("afterLines", false), ("afterAutospacing", true)]),
    ] {
        match p {
            Prop::Keep => {}
            Prop::Set(t) => {
                set.push((w(attr), t.to_string()));
                supersede(&mut set, &mut remove, "spacing", &sup)?;
            }
            // Clear returns to the inherited value: the superseding attributes go too
            // (review of PR #59, m2), as a font slot's Clear removes its theme attribute
            Prop::Clear => {
                remove.push(w(attr));
                remove.extend(sup.iter().map(|(a, _)| w(a)));
            }
        }
    }
    match f.line_spacing {
        Prop::Keep => {}
        Prop::Set(l) => {
            let (v, rule) = match l {
                LineSpacing::Auto(v) => (v, "auto"),
                LineSpacing::Exact(v) => (v, "exact"),
                LineSpacing::AtLeast(v) => (v, "atLeast"),
            };
            set.push((w("line"), v.to_string()));
            let rule_absent = ppr
                .and_then(|p| p.elements().find(|e| e.is(SpacingBetweenLines::QNAME)))
                .is_some_and(|sp| sp.attr(w("lineRule")).is_none());
            // `auto` is lineRule's default: an element without it already reads as auto
            // (review of PR #59, n2)
            if !(rule == "auto" && rule_absent) {
                set.push((w("lineRule"), rule.into()));
            }
        }
        Prop::Clear => remove.extend([w("line"), w("lineRule")]),
    }
    pl.attrs(Kind::Spacing, set, remove)?;
    // indentation
    let mut set = Vec::new();
    let mut remove = Vec::new();
    for (p, attr, plain, chars) in [
        (&f.indent_left, "left", "start", [("leftChars", false), ("startChars", false)]),
        (&f.indent_right, "right", "end", [("rightChars", false), ("endChars", false)]),
    ] {
        match p {
            Prop::Keep => {}
            Prop::Set(t) => {
                set.push((w(attr), t.to_string()));
                remove.push(w(plain));
                supersede(&mut set, &mut remove, "ind", &chars)?;
            }
            Prop::Clear => {
                remove.extend([w(attr), w(plain)]);
                remove.extend(chars.iter().map(|(a, _)| w(a)));
            }
        }
    }
    match f.first_line {
        Prop::Keep => {}
        Prop::Set(fl) => {
            let (attr, other, v) = match fl {
                FirstLine::Indent(v) => ("firstLine", "hanging", v),
                FirstLine::Hanging(v) => ("hanging", "firstLine", v),
            };
            set.push((w(attr), v.to_string()));
            remove.push(w(other));
            supersede(&mut set, &mut remove, "ind", &[("firstLineChars", false), ("hangingChars", false)])?;
        }
        Prop::Clear => remove.extend([w("firstLine"), w("hanging"), w("firstLineChars"), w("hangingChars")]),
    }
    pl.attrs(Kind::Ind, set, remove)?;
    match f.alignment {
        Prop::Keep => {}
        Prop::Set(a) => pl.val(Kind::Jc, Prop::Set(a.as_str().into()), &[])?,
        Prop::Clear => pl.clear(Kind::Jc)?,
    }
    Ok(pl.plan)
}

// ---------------------------------------------------------------------
// Dry run and write
// ---------------------------------------------------------------------

/// Item index where `insert::<C>()` would put a new child `q` of a parent
/// with content `E`: before the first child of a later slot.
pub(crate) fn slot_index<E: ElementSpec>(parent: &RawNode, q: QName) -> usize {
    let Content::Slots(slots) = E::CONTENT else { return parent.children().len() };
    let Some(slot) = slots.iter().position(|s| s.contains(q)) else { return parent.children().len() };
    parent
        .indexed_elements()
        .find(|(_, c)| slots.iter().position(|s| s.names.iter().any(|n| c.is(*n))).is_some_and(|r| r > slot))
        .map_or(parent.children().len(), |(i, _)| i)
}

/// Is the WML namespace bound to a non-empty prefix in scope at the element
/// at `path` (from `root`), declarations on `extra` (a new child) included?
/// `set_attr` needs one for a new `w:` attribute (review n4).
pub(crate) fn wml_prefix_in_scope(root: &RawNode, path: &[usize]) -> bool {
    prefix_in_scope(root, path, NS_W)
}

/// Is `uri` bound to a non-empty prefix in scope at the element at `path`?
pub(crate) fn prefix_in_scope(root: &RawNode, path: &[usize], uri: &str) -> bool {
    let mut decls: Vec<(Option<String>, String)> = Vec::new();
    let mut node = root;
    let push = |n: &RawNode, decls: &mut Vec<(Option<String>, String)>| {
        for a in n.attrs().iter().filter(|a| a.name().is_namespace_declaration()) {
            decls.push((a.name().declared_prefix().map(str::to_owned), a.value().to_owned()));
        }
    };
    push(node, &mut decls);
    for &i in path {
        node = node.children()[i].as_element().expect("paths point at elements");
        push(node, &mut decls);
    }
    let effective = |p: &str| decls.iter().rev().find(|(q, _)| q.as_deref() == Some(p)).map(|(_, u)| u.as_str());
    decls.iter().any(|(p, u)| u == uri && p.as_deref().is_some_and(|p| !p.is_empty() && effective(p) == Some(uri)))
}

macro_rules! dispatch {
    ($kind:expr, $f:ident $args:tt; $($k:ident => $t:ty),* $(,)?) => {
        match $kind { $(Kind::$k => $f::<_, $t> $args,)* _ => unreachable!("kind not in this container") }
    };
}

fn check_one<E: ElementSpec, C: ChildOf<E>>(parent: &LensMut<'_, E>, idx: usize) -> ooxml_core::Result<()> {
    parent.check_insert_at::<C>(idx)
}

fn apply_one<E: ElementSpec, C: ChildOf<E>>(parent: &mut LensMut<'_, E>, op: &PropPlan) -> Result<()> {
    if op.remove_element {
        parent.remove::<C>();
        return Ok(());
    }
    let mut c = parent.get_or_insert::<C>()?;
    for q in &op.remove {
        c.remove_attr(*q)?;
    }
    for (q, v) in &op.set {
        c.set_attr(*q, v)?;
    }
    Ok(())
}

fn check_rpr(rpr: &LensMut<'_, RunProperties>, op: &PropPlan) -> ooxml_core::Result<()> {
    let idx = slot_index::<RunProperties>(rpr.node(), op.kind.qname());
    dispatch!(op.kind, check_one (rpr, idx);
        RStyle => RunStyle, RFonts => RunFonts, B => Bold, BCs => BoldComplexScript, I => Italic,
        ICs => ItalicComplexScript, Strike => Strike, Color => ColorEl, Sz => FontSize,
        SzCs => FontSizeComplexScript, Highlight => HighlightEl, U => UnderlineEl, VertAlign => VerticalTextAlignment)
}

pub(crate) fn apply_rpr(rpr: &mut LensMut<'_, RunProperties>, op: &PropPlan) -> Result<()> {
    dispatch!(op.kind, apply_one (rpr, op);
        RStyle => RunStyle, RFonts => RunFonts, B => Bold, BCs => BoldComplexScript, I => Italic,
        ICs => ItalicComplexScript, Strike => Strike, Color => ColorEl, Sz => FontSize,
        SzCs => FontSizeComplexScript, Highlight => HighlightEl, U => UnderlineEl, VertAlign => VerticalTextAlignment)
}

fn check_ppr(ppr: &LensMut<'_, ParagraphProperties>, op: &PropPlan) -> ooxml_core::Result<()> {
    let idx = slot_index::<ParagraphProperties>(ppr.node(), op.kind.qname());
    dispatch!(op.kind, check_one (ppr, idx);
        PStyle => ParagraphStyleId, KeepNext => KeepNext, KeepLines => KeepLines, Spacing => SpacingBetweenLines,
        Ind => Indentation, Jc => Justification)
}

pub(crate) fn apply_ppr(ppr: &mut LensMut<'_, ParagraphProperties>, op: &PropPlan) -> Result<()> {
    dispatch!(op.kind, apply_one (ppr, op);
        PStyle => ParagraphStyleId, KeepNext => KeepNext, KeepLines => KeepLines, Spacing => SpacingBetweenLines,
        Ind => Indentation, Jc => Justification)
}

/// Does any planned write add a `w:` attribute that is not there yet (so
/// `set_attr` will need a prefixed WML binding)?
fn adds_attribute(container: Option<&RawNode>, op: &PropPlan) -> bool {
    let el = op.existing.and_then(|i| container?.children().get(i)?.as_element());
    op.set.iter().any(|(q, _)| el.is_none_or(|e| e.attr(*q).is_none()))
}

impl Document {
    /// R1, R2 (incl. a tracked formatting change on the run) and R3 for a
    /// run to format (ADR-0007 §4 "Targets"; R4 is not required).
    pub(crate) fn check_formattable_run(&self, at: RunRef) -> Result<()> {
        let r = self.run_at(at).ok_or(Error::NoSuchRun { paragraph: at.para.index, run: at.run })?;
        match r.check_settable() {
            Err(Error::InsideRevision { context: RunContext::Direct }) if r.has_formatting_change() => {
                return Err(Error::TrackedPropertyChange { element: "w:rPrChange" });
            }
            Ok(()) | Err(Error::RunNotTextOnly { .. }) => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// ADR-0007 §4 "Targets" for a paragraph.
    pub(crate) fn check_formattable_paragraph(&self, at: ParaRef) -> Result<()> {
        let p = self.paragraph_at(at).ok_or(Error::NoSuchParagraph(at.index))?;
        if let Some(w) = self.stories[at.story.0].index.wrappers(p.info.frame).into_iter().find(|w| *w != Wrapper::Hyperlink) {
            return Err(Error::UnsupportedContext { ancestor: w.qname() });
        }
        if p.runs().any(|r| r.in_field()) {
            return Err(Error::InsideField);
        }
        if let Some(ppr) = p.raw().elements().find(|e| is::<ParagraphProperties>(e)) {
            if ppr.elements().any(|e| e.is(w("pPrChange"))) {
                return Err(Error::TrackedPropertyChange { element: "w:pPrChange" });
            }
            let mark = ppr.elements().find(|e| e.is(w("rPr")));
            for (local, context) in [
                ("ins", RunContext::Inserted),
                ("del", RunContext::Deleted),
                ("moveFrom", RunContext::MovedFrom),
                ("moveTo", RunContext::MovedTo),
            ] {
                if mark.is_some_and(|m| m.elements().any(|e| e.is(w(local)))) {
                    return Err(Error::InsideRevision { context });
                }
            }
            if mark.is_some_and(|m| m.elements().any(|e| e.is(RPR_CHANGE))) {
                return Err(Error::TrackedPropertyChange { element: "w:rPrChange" });
            }
        }
        if p.info.in_revision_range {
            return Err(Error::InsideRevision { context: RunContext::Direct });
        }
        let path = self.stories[at.story.0].index.path(p.info.frame);
        crate::revisions::check_path(self.story_root_node(at.story), &path)?;
        Ok(())
    }

    /// The run's `w:rPr`, its path from the story root, and the run's path.
    pub(crate) fn run_container(&self, at: RunRef) -> Result<(Option<&RawNode>, Vec<usize>)> {
        let missing = || Error::NoSuchRun { paragraph: at.para.index, run: at.run };
        let data = self.stories.get(at.para.story.0).ok_or(Error::NoSuchStory)?;
        let info = data.index.paragraphs.get(at.para.index).and_then(|p| p.runs.get(at.run)).ok_or_else(missing)?;
        let path = data.index.path(info.frame);
        let r = node_at(self.story_root_node(at.para.story), &path);
        if r.elements().any(|e| e.is(MC_ALTERNATE_CONTENT)) {
            return Err(Error::Core(ooxml_core::Error::UnresolvedSibling(R::QNAME)));
        }
        Ok((r.elements().find(|e| is::<RunProperties>(e)), path))
    }

    pub(crate) fn para_container(&self, at: ParaRef) -> Result<(Option<&RawNode>, Vec<usize>)> {
        let data = self.stories.get(at.story.0).ok_or(Error::NoSuchStory)?;
        let info = data.index.paragraphs.get(at.index).ok_or(Error::NoSuchParagraph(at.index))?;
        let path = data.index.path(info.frame);
        let p = node_at(self.story_root_node(at.story), &path);
        Ok((p.elements().find(|e| is::<ParagraphProperties>(e)), path))
    }

    /// n4: every new `w:` attribute will find a prefixed WML binding.
    pub(crate) fn check_prefixes(&self, story: crate::story::StoryIndex, path: &[usize], container: Option<&RawNode>, plan: &ContainerPlan) -> Result<()> {
        let root = self.story_root_node(story);
        let parent = node_at(root, path);
        let cpath: Vec<usize> = match container {
            Some(c) => {
                let i = parent.children().iter().position(|x| x.as_element().is_some_and(|e| std::ptr::eq(e, c))).expect("child");
                path.iter().copied().chain([i]).collect()
            }
            None => path.to_vec(),
        };
        for op in plan.props.iter().filter(|op| adds_attribute(container, op)) {
            let p: Vec<usize> = match op.existing {
                Some(i) => cpath.iter().copied().chain([i]).collect(),
                None => cpath.clone(),
            };
            if !wml_prefix_in_scope(root, &p) {
                return Err(Error::Core(ooxml_core::Error::NamespaceNotInScope(NS_W.into())));
            }
        }
        Ok(())
    }

    /// Plans, dry-runs and writes run formatting on several runs atomically.
    fn format_runs(&mut self, runs: &[RunRef], f: &RunFormat) -> Result<()> {
        self.write_gate_for(Operation::Format)?;
        let mut plans = Vec::new();
        for &at in runs {
            self.check_formattable_run(at)?;
            let (rpr, path) = self.run_container(at)?;
            let plan = plan_run(rpr, f)?;
            if plan.props.is_empty() {
                continue;
            }
            self.check_prefixes(at.para.story, &path, rpr, &plan)?;
            plans.push((at, plan));
        }
        self.write_run_plans(plans)
    }

    pub(crate) fn write_run_plans(&mut self, plans: Vec<(RunRef, ContainerPlan)>) -> Result<()> {
        for (at, plan) in &plans {
            self.with_run(*at, |r| {
                match r.child_mut::<RunProperties>() {
                    Some(rpr) => {
                        for op in &plan.props {
                            if op.existing.is_none() && !op.remove_element {
                                check_rpr(&rpr, op)?;
                            }
                        }
                    }
                    None => {
                        let idx = slot_index::<R>(r.node(), RunProperties::QNAME);
                        r.check_insert_at::<RunProperties>(idx)?;
                    }
                }
                Ok(())
            })?;
        }
        let mut touched = std::collections::BTreeSet::new();
        for (at, plan) in &plans {
            self.with_run(*at, |r| {
                let emptied = {
                    let mut rpr = r.get_or_insert::<RunProperties>()?;
                    for op in &plan.props {
                        apply_rpr(&mut rpr, op)?;
                    }
                    plan.removes_only() && is_empty_container(rpr.node())
                };
                // a container a Clear left empty goes too (review of PR #59, n1)
                if emptied {
                    r.remove::<RunProperties>();
                }
                Ok(())
            })?;
            touched.insert(at.para);
        }
        for p in touched {
            self.bump(p);
            self.reindex(p.story);
        }
        Ok(())
    }

    /// Writes direct run properties (ADR-0007 §4) on one run. Writing a value
    /// the run already has is a no-op (byte-identical output); setting a
    /// property removes the attributes that would override it (theme colour
    /// and font attributes).
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Format`]); R1–R3 of the run ([`Error::InsideField`],
    /// [`Error::InsideRevision`], [`Error::UnsupportedContext`]),
    /// [`Error::TrackedPropertyChange`]; [`Error::DuplicateProperty`];
    /// `Core(UnresolvedSibling)` (producer order or `mc:AlternateContent`)
    /// and other dry-run failures; `Core(NamespaceNotInScope)` when the part
    /// binds WML only as the default namespace; [`Error::InvalidValue`].
    pub fn format_run(&mut self, run: RunRef, f: &RunFormat) -> Result<()> {
        self.format_runs(&[run], f)
    }

    /// [`Self::format_run`] on every run of the paragraph, atomically.
    pub fn format_paragraph_runs(&mut self, p: ParaRef, f: &RunFormat) -> Result<()> {
        let para = self.paragraph_at(p).ok_or(Error::NoSuchParagraph(p.index))?;
        let runs: Vec<RunRef> = para.runs().map(|r| r.run_ref()).collect();
        self.format_runs(&runs, f)
    }

    /// Writes direct paragraph properties (ADR-0007 §4). Setting an indent or
    /// spacing value removes the direct attributes that supersede it
    /// (`*Chars`, `*Lines`, `*Autospacing`, `start`/`end`) and writes `0`
    /// for those inherited from the style chain.
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Format`]); [`Error::UnsupportedContext`] (block-level
    /// content control or custom XML), [`Error::InsideField`] (a run of it
    /// is in a field), [`Error::TrackedPropertyChange`] (`w:pPrChange`, or a
    /// mark `w:rPrChange`), [`Error::InsideRevision`] (a tracked paragraph
    /// mark); [`Error::DuplicateProperty`]; [`Error::StyleChainTooDeep`];
    /// dry-run failures as for [`Self::format_run`].
    pub fn format_paragraph(&mut self, p: ParaRef, f: &ParagraphFormat) -> Result<()> {
        self.write_gate_for(Operation::Format)?;
        self.check_formattable_paragraph(p)?;
        let (ppr, path) = self.para_container(p)?;
        let style = ppr
            .and_then(|e| e.elements().find(|c| c.is(ParagraphStyleId::QNAME)))
            .and_then(|s| s.attr(VAL))
            .map(str::to_owned);
        let inherited = |el: &'static str, attr: &'static str| self.inherited_ppr_attr(style.as_deref(), el, attr);
        let plan = plan_paragraph(ppr, f, &inherited)?;
        if plan.props.is_empty() {
            return Ok(());
        }
        self.check_prefixes(p.story, &path, ppr, &plan)?;
        self.write_para_plan(p, &plan)
    }

    pub(crate) fn write_para_plan(&mut self, p: ParaRef, plan: &ContainerPlan) -> Result<()> {
        self.with_para(p, |para| {
            if para.node().elements().any(is::<ParagraphProperties>) {
                let ppr = para.child_mut::<ParagraphProperties>().expect("exists");
                for op in &plan.props {
                    if op.existing.is_none() && !op.remove_element {
                        check_ppr(&ppr, op)?;
                    }
                }
            } else {
                para.check_insert_at::<ParagraphProperties>(slot_index::<P>(para.node(), ParagraphProperties::QNAME))?;
            }
            Ok(())
        })?;
        self.with_para(p, |para| {
            let emptied = {
                let mut ppr = para.get_or_insert::<ParagraphProperties>()?;
                for op in &plan.props {
                    apply_ppr(&mut ppr, op)?;
                }
                plan.removes_only() && is_empty_container(ppr.node())
            };
            if emptied {
                para.remove::<ParagraphProperties>();
            }
            Ok(())
        })?;
        self.bump(p);
        self.reindex(p.story);
        Ok(())
    }

    /// Style setters (§5) reuse the planner for `w:pStyle` / `w:rStyle`.
    pub(crate) fn plan_style_ref(&self, target: StyleTarget, id: Option<&str>) -> Result<Option<ContainerPlan>> {
        let (container, kind) = match target {
            StyleTarget::Para(p) => (self.para_container(p)?.0, Kind::PStyle),
            StyleTarget::Run(r) => (self.run_container(r)?.0, Kind::RStyle),
        };
        let mut pl = Planner::new(container);
        match id {
            Some(id) => pl.val(kind, Prop::Set(id.to_owned()), &[])?,
            None => pl.val(kind, Prop::Clear, &[])?,
        }
        Ok((!pl.plan.props.is_empty()).then_some(pl.plan))
    }

    pub(crate) fn write_style_ref(&mut self, target: StyleTarget, plan: ContainerPlan) -> Result<()> {
        match target {
            StyleTarget::Para(p) => {
                self.check_formattable_paragraph(p)?;
                let (ppr, path) = self.para_container(p)?;
                self.check_prefixes(p.story, &path, ppr, &plan)?;
                self.write_para_plan(p, &plan)
            }
            StyleTarget::Run(r) => {
                self.check_formattable_run(r)?;
                let (rpr, path) = self.run_container(r)?;
                self.check_prefixes(r.para.story, &path, rpr, &plan)?;
                self.write_run_plans(vec![(r, plan)])
            }
        }
    }
}

/// What a style reference is written on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StyleTarget {
    Para(ParaRef),
    Run(RunRef),
}

