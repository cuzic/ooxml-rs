//! Minimal editing of the theme reached from a slide master (ADR-0016 §10).

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ChildOf, ElementSpec, LensMut, Package, QName, RawNode, RegistryPlan};
use ooxml_schema::dml;

use super::formatting::remove_choices;
use super::{ColorSpec, SystemColor, WriteColor};
use crate::deck::{a, child, p, rel, un};
use crate::names::R_NS;
use crate::{ChainLink, Error, Presentation, Result};

const CT_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const CT_NOTES_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const CT_HANDOUT_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.handoutMaster+xml";

/// A physical slot in the theme colour scheme.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeColorSlot {
    /// Dark 1.
    Dk1,
    /// Light 1.
    Lt1,
    /// Dark 2.
    Dk2,
    /// Light 2.
    Lt2,
    /// Accent 1.
    Accent1,
    /// Accent 2.
    Accent2,
    /// Accent 3.
    Accent3,
    /// Accent 4.
    Accent4,
    /// Accent 5.
    Accent5,
    /// Accent 6.
    Accent6,
    /// Hyperlink.
    Hlink,
    /// Followed hyperlink.
    FolHlink,
}

/// The colour to write to a theme slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeColor {
    /// An sRGB triplet.
    Rgb([u8; 3]),
    /// A system colour and its optional sRGB fallback.
    System(SystemColor, Option<[u8; 3]>),
}

impl From<ThemeColor> for WriteColor {
    fn from(color: ThemeColor) -> Self {
        match color {
            ThemeColor::Rgb(rgb) => Self::Rgb(rgb),
            ThemeColor::System(color, fallback) => Self::System(color, fallback),
        }
    }
}

/// The three typeface attributes in a major or minor theme font collection.
///
/// Each setter replaces all three values. An empty string clears the
/// corresponding typeface, including the empty strings supplied by
/// `ThemeFonts::default()`. Use literal font names. Theme references such as
/// `+mj-lt` are rejected.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThemeFonts<'a> {
    /// Latin typeface.
    pub latin: &'a str,
    /// East Asian typeface.
    pub east_asian: &'a str,
    /// Complex-script typeface.
    pub complex_script: &'a str,
}

/// Options for opening a slide master's theme for editing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThemeOptions {
    /// Allow editing a theme also used by another slide, notes or handout master.
    pub allow_shared: bool,
}

/// A theme reached from a slide master. Each setter stages one edited clone
/// in a registry transaction.
#[derive(Debug)]
pub struct ThemeMut<'a> {
    pkg: &'a mut Package,
    part: String,
}

impl Presentation {
    /// Opens the theme of the zero-based slide master in `p:sldMasterIdLst`.
    /// Refuses themes shared with another slide, notes or handout master.
    pub fn master_theme_mut(&mut self, master: usize) -> Result<ThemeMut<'_>> {
        self.master_theme_mut_with(master, ThemeOptions::default())
    }

    /// Opens the same theme, with explicit permission to edit shared themes.
    pub fn master_theme_mut_with(
        &mut self,
        master: usize,
        options: ThemeOptions,
    ) -> Result<ThemeMut<'_>> {
        let main = self.main.clone();
        let rels = self.pkg.relationships(&main)?;
        let root = self.pkg.xml_part(&main)?.root();
        let rid = child(root, p("sldMasterIdLst"))
            .and_then(|list| {
                list.elements()
                    .filter(|node| node.is(p("sldMasterId")))
                    .nth(master)
            })
            .ok_or(Error::MasterIndexOutOfRange { index: master })?
            .attr(QName::new(R_NS, "id"));
        let master = rid
            .and_then(|rid| rels.by_id(rid))
            .filter(|edge| edge.rel_type == rel::MASTER)
            .and_then(|edge| rels.resolve(edge))
            .and_then(|name| self.pkg.part_name(&name))
            .ok_or_else(|| Error::BrokenChain {
                from: main.clone(),
                missing: ChainLink::Master,
            })?
            .to_owned();

        let rels = self.pkg.relationships(&master)?;
        let mut themes = rels.by_type(rel::THEME);
        let part = themes
            .next()
            .filter(|_| themes.next().is_none())
            .and_then(|edge| rels.resolve(edge))
            .and_then(|name| self.pkg.part_name(&name))
            .ok_or_else(|| Error::BrokenChain {
                from: master.clone(),
                missing: ChainLink::Theme,
            })?
            .to_owned();

        if !options.allow_shared
            && self.pkg.incoming_relationships(&part)?.iter().any(|edge| {
                edge.rel_type == rel::THEME
                    && edge.source.as_deref().is_some_and(|source| {
                        self.pkg.part_name(source) != Some(master.as_str())
                            && matches!(
                                self.pkg.content_type(source),
                                Some(CT_MASTER | CT_NOTES_MASTER | CT_HANDOUT_MASTER)
                            )
                    })
            })
        {
            return Err(Error::SharedTheme);
        }
        self.pkg.xml_part(&part)?.lens::<dml::Theme>()?;
        Ok(ThemeMut {
            pkg: &mut self.pkg,
            part,
        })
    }
}

impl ThemeMut<'_> {
    /// Changes one colour in the base theme.
    pub fn set_color(&mut self, slot: ThemeColorSlot, value: ThemeColor) -> Result<()> {
        self.edit(|mut elements| {
            let mut scheme = elements
                .child_mut::<dml::ColorScheme>()
                .ok_or_else(incomplete_theme)?;
            macro_rules! slots {
                ($($slot:ident: $spec:ident),* $(,)?) => {
                    match slot {
                        $(ThemeColorSlot::$slot => {
                            if scheme.node().elements()
                                .filter(|node| node.is(MC_ALTERNATE_CONTENT))
                                .flat_map(RawNode::elements)
                                .any(|branch| child(branch, dml::$spec::QNAME).is_some())
                            {
                                return Err(alternate_color());
                            }
                            let mut node = scheme.child_mut::<dml::$spec>().ok_or_else(incomplete_theme)?;
                            set_color(&mut node, value)
                        })*
                    }
                };
            }
            slots! {
                Dk1: Dark1Color,
                Lt1: Light1Color,
                Dk2: Dark2Color,
                Lt2: Light2Color,
                Accent1: Accent1Color,
                Accent2: Accent2Color,
                Accent3: Accent3Color,
                Accent4: Accent4Color,
                Accent5: Accent5Color,
                Accent6: Accent6Color,
                Hlink: Hyperlink,
                FolHlink: FollowedHyperlinkColor,
            }
        })
    }

    /// Changes the major Latin, East Asian and complex-script typefaces.
    pub fn set_major_font(&mut self, fonts: ThemeFonts<'_>) -> Result<()> {
        self.set_fonts::<dml::MajorFont>(fonts)
    }

    /// Changes the minor Latin, East Asian and complex-script typefaces.
    pub fn set_minor_font(&mut self, fonts: ThemeFonts<'_>) -> Result<()> {
        self.set_fonts::<dml::MinorFont>(fonts)
    }

    fn set_fonts<E: ChildOf<dml::FontScheme> + ElementSpec>(
        &mut self,
        fonts: ThemeFonts<'_>,
    ) -> Result<()>
    where
        dml::LatinFont: ChildOf<E>,
        dml::EastAsianFont: ChildOf<E>,
        dml::ComplexScriptFont: ChildOf<E>,
    {
        self.edit(|mut elements| {
            let mut scheme = elements
                .child_mut::<dml::FontScheme>()
                .ok_or_else(incomplete_theme)?;
            let mut collection = scheme.child_mut::<E>().ok_or_else(incomplete_theme)?;
            set_typeface(
                collection
                    .child_mut::<dml::LatinFont>()
                    .ok_or_else(incomplete_theme)?,
                fonts.latin,
            )?;
            set_typeface(
                collection
                    .child_mut::<dml::EastAsianFont>()
                    .ok_or_else(incomplete_theme)?,
                fonts.east_asian,
            )?;
            set_typeface(
                collection
                    .child_mut::<dml::ComplexScriptFont>()
                    .ok_or_else(incomplete_theme)?,
                fonts.complex_script,
            )
        })
    }

    fn edit(
        &mut self,
        edit: impl FnOnce(LensMut<'_, dml::ThemeElements>) -> Result<()>,
    ) -> Result<()> {
        let mut doc = self.pkg.xml_part(&self.part)?.clone();
        let mut theme = doc.lens_mut::<dml::Theme>()?;
        edit(
            theme
                .child_mut::<dml::ThemeElements>()
                .ok_or_else(incomplete_theme)?,
        )?;
        if doc.is_dirty() {
            let mut plan = RegistryPlan::new(self.pkg);
            plan.stage_part(self.pkg, &self.part, doc)?;
            self.pkg.apply(plan)?;
        }
        Ok(())
    }
}

fn incomplete_theme() -> Error {
    Error::UnsupportedContent("theme is missing a required colour or font element".into())
}

fn alternate_color() -> Error {
    Error::UnsupportedContent("theme colour slot or value is wrapped in mc:AlternateContent".into())
}

fn set_typeface<E: ElementSpec>(mut node: LensMut<'_, E>, value: &str) -> Result<()> {
    if value.starts_with("+mj-") || value.starts_with("+mn-") {
        return Err(Error::ValueOutOfRange { what: "typeface" });
    }
    let attr = un("typeface");
    if node.node().attr(attr) != Some(value) {
        node.preserve_attribute_spelling();
        node.set_attr(attr, value)?;
    }
    Ok(())
}

fn set_color<E: ElementSpec>(node: &mut LensMut<'_, E>, color: ThemeColor) -> Result<()>
where
    dml::RgbColorModelHex: ChildOf<E>,
    dml::SystemColor: ChildOf<E>,
{
    if child(node.node(), MC_ALTERNATE_CONTENT).is_some() {
        return Err(alternate_color());
    }
    let spec = ColorSpec::from(WriteColor::from(color));
    if spec.matches(node.node()) {
        return Ok(());
    }
    remove_choices(
        node,
        &[
            a("srgbClr"),
            a("sysClr"),
            a("scrgbClr"),
            a("hslClr"),
            a("prstClr"),
        ],
    );
    match color {
        ThemeColor::Rgb([r, g, b]) => {
            let mut rgb = dml::RgbColorModelHexViewMut(node.insert::<dml::RgbColorModelHex>()?);
            rgb.set_val(&format!("{r:02X}{g:02X}{b:02X}"))?;
        }
        ThemeColor::System(color, fallback) => {
            let mut sys = dml::SystemColorViewMut(node.insert::<dml::SystemColor>()?);
            sys.set_val(color)?;
            if let Some([r, g, b]) = fallback {
                sys.set_last_color(&format!("{r:02X}{g:02X}{b:02X}"))?;
            }
        }
    }
    Ok(())
}
