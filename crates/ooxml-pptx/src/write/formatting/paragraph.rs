use super::*;
use crate::Spacing;
use ooxml_core::{EnumType, RegistryPlan, RegistryRefusal};

fn spacing_matches(value: &Spacing, node: &RawNode) -> bool {
    let Some(child) = only_choice(
        Some(node),
        &[dml::SpacingPercent::QNAME, dml::SpacingPoints::QNAME],
    ) else {
        return false;
    };
    match value {
        Spacing::Percent(value) => {
            child.is(dml::SpacingPercent::QNAME)
                && child
                    .attr(dml::SpacingPercent::ATTR_VAL)
                    .and_then(parse_dml_percent)
                    .is_some_and(|v| v.percent() == value.percent())
        }
        Spacing::Points(value) => {
            child.is(dml::SpacingPoints::QNAME)
                && child
                    .attr(dml::SpacingPoints::ATTR_VAL)
                    .and_then(|raw| raw.parse().ok())
                    == Some(*value)
        }
    }
}

fn validate_spacing(spacing: Spacing) -> Result<()> {
    match spacing {
        Spacing::Percent(value) if (0.0..=13_200.0).contains(&value.percent()) => Ok(()),
        Spacing::Points(value) if (0..=158_400).contains(&value) => Ok(()),
        _ => Err(Error::ValueOutOfRange { what: "spacing" }),
    }
}

fn write_spacing<E: ElementSpec>(lens: &mut LensMut<'_, E>, value: Spacing) -> Result<()>
where
    dml::SpacingPercent: ChildOf<E>,
    dml::SpacingPoints: ChildOf<E>,
{
    match value {
        Spacing::Percent(value) => {
            dml::SpacingPercentViewMut(lens.insert::<dml::SpacingPercent>()?).set_val(value)?
        }
        Spacing::Points(value) => {
            let value =
                i32::try_from(value).map_err(|_| Error::ValueOutOfRange { what: "spacing" })?;
            dml::SpacingPointsViewMut(lens.insert::<dml::SpacingPoints>()?).set_val(value)?;
        }
    }
    Ok(())
}

macro_rules! paragraph_spacing {
    ($set:ident, $clear:ident, $element:ident) => {
        impl ParagraphMut<'_> {
            #[doc = concat!("Sets direct paragraph ", stringify!($set), ". Cached autofit values are retained until refit.")]
            pub fn $set(&mut self, value: Spacing) -> Result<()> {
                validate_spacing(value)?;
                self.update_properties(
                    |node| only_choice(node, &[dml::$element::QNAME]).is_none_or(|node| !spacing_matches(&value, node)),
                    |mut view| {
                        remove_choices(&mut view.0, &[dml::$element::QNAME]);
                        write_spacing(&mut view.0.insert::<dml::$element>()?, value)
                    },
                )
            }

            #[doc = concat!("Clears direct paragraph ", stringify!($set), ".")]
            pub fn $clear(&mut self) -> Result<()> {
                let names = [dml::$element::QNAME];
                self.update_properties(
                    |node| changes_choice::<()>(node, &names, &Some(FormatChange::Clear), |_, _| false),
                    |mut view| { remove_choices(&mut view.0, &names); Ok(()) },
                )
            }
        }
    };
}

paragraph_spacing!(line_spacing, clear_line_spacing, LineSpacing);
paragraph_spacing!(space_before, clear_space_before, SpaceBefore);
paragraph_spacing!(space_after, clear_space_after, SpaceAfter);

/// A direct paragraph tab stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabStop {
    /// Position in EMU.
    pub position: i32,
    /// Alignment at this stop.
    pub alignment: dml::TextTabAlignmentValues,
}

fn tabs_match(stops: &[TabStop], node: &RawNode) -> bool {
    node.elements().count() == stops.len()
        && node.elements().zip(stops).all(|(node, stop)| {
            node.is(dml::TabStop::QNAME)
                && node.attr(dml::TabStop::ATTR_POS).and_then(int) == Some(stop.position)
                && node
                    .attr(dml::TabStop::ATTR_ALGN)
                    .and_then(dml::TextTabAlignmentValues::parse)
                    == Some(stop.alignment)
        })
}

impl ParagraphMut<'_> {
    /// Replaces the direct tab list. An empty list explicitly supplies zero stops.
    pub fn tab_stops(&mut self, stops: &[TabStop]) -> Result<()> {
        if stops.len() > 32 {
            return Err(Error::ValueOutOfRange { what: "tabLst" });
        }
        self.update_properties(
            |node| {
                only_choice(node, &[dml::TabStopList::QNAME])
                    .is_none_or(|node| !tabs_match(stops, node))
            },
            |mut view| {
                remove_choices(&mut view.0, &[dml::TabStopList::QNAME]);
                let mut list = view.0.insert::<dml::TabStopList>()?;
                for stop in stops {
                    let mut tab = dml::TabStopViewMut(list.insert::<dml::TabStop>()?);
                    tab.set_position(stop.position)?;
                    tab.set_alignment(stop.alignment)?;
                }
                Ok(())
            },
        )
    }

    /// Clears the direct tab list so tabs can be inherited.
    pub fn clear_tab_stops(&mut self) -> Result<()> {
        let names = [dml::TabStopList::QNAME];
        self.update_properties(
            |node| changes_choice::<()>(node, &names, &Some(FormatChange::Clear), |_, _| false),
            |mut view| {
                remove_choices(&mut view.0, &names);
                Ok(())
            },
        )
    }
}

/// The size of an explicit character bullet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BulletSize {
    /// Follow the paragraph text size.
    FollowText,
    /// Percentage of the text size (25–400%).
    Percent(DmlPercent),
    /// Size in hundredths of a point.
    Points(i32),
}

/// A direct paragraph bullet.
#[derive(Clone, Debug, PartialEq)]
pub enum BulletSpec {
    /// Explicitly suppress bullets.
    None,
    /// Use a character bullet.
    Char {
        /// The bullet character.
        char: char,
        /// Direct typeface.
        font: Option<FontRef>,
        /// Direct size.
        size: Option<BulletSize>,
        /// Direct colour.
        color: Option<ColorSpec>,
    },
    /// Use automatic numbering.
    AutoNumber {
        /// Numbering scheme.
        scheme: dml::TextAutoNumberSchemeValues,
        /// First number.
        start_at: i32,
    },
    /// Clear the direct bullet and its appearance.
    Inherit,
    /// Picture bullets require an image relationship and cannot be created here.
    Picture,
}

const BULLET_COLOR: [QName; 2] = [a("buClrTx"), a("buClr")];
const BULLET_FONT: [QName; 2] = [a("buFontTx"), a("buFont")];
const BULLET_SIZE: [QName; 3] = [a("buSzTx"), a("buSzPct"), a("buSzPts")];
const BULLET_TYPE: [QName; 4] = [a("buNone"), a("buAutoNum"), a("buChar"), a("buBlip")];

fn has_choice(node: Option<&RawNode>, names: &[QName]) -> bool {
    node.is_some_and(|node| {
        node.elements()
            .any(|child| names.iter().any(|&name| child.is(name)))
    })
}

fn size_matches(size: &BulletSize, child: &RawNode) -> bool {
    match size {
        BulletSize::FollowText => child.is(dml::BulletSizeText::QNAME),
        BulletSize::Percent(value) => {
            child.is(dml::BulletSizePercentage::QNAME)
                && child
                    .attr(dml::BulletSizePercentage::ATTR_VAL)
                    .and_then(parse_dml_percent)
                    .is_some_and(|v| v.percent() == value.percent())
        }
        BulletSize::Points(value) => {
            child.is(dml::BulletSizePoints::QNAME)
                && child.attr(dml::BulletSizePoints::ATTR_VAL).and_then(int) == Some(*value)
        }
    }
}

impl BulletSpec {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Picture => Err(Error::UnsupportedBullet),
            Self::AutoNumber { start_at, .. } => check_range(*start_at, 1, 32_767, "startAt"),
            Self::Char {
                char,
                font,
                size,
                color,
            } => {
                if !ooxml_core::value::is_xml_char(*char) {
                    return Err(Error::InvalidXmlChar {
                        ch: *char,
                        offset: 0,
                    });
                }
                if let Some(font) = font {
                    check_attribute_text(font.typeface()?)?;
                }
                if let Some(color) = color {
                    color.validate()?;
                }
                match size {
                    Some(BulletSize::Points(value)) => check_range(*value, 100, 400_000, "buSzPts"),
                    Some(BulletSize::Percent(value))
                        if !(25.0..=400.0).contains(&value.percent()) =>
                    {
                        Err(Error::ValueOutOfRange { what: "buSzPct" })
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    fn matches(&self, node: Option<&RawNode>) -> bool {
        let kind = only_choice(node, &BULLET_TYPE);
        let (font, size, color) = match self {
            Self::Char {
                font, size, color, ..
            } => (font.as_ref(), size.as_ref(), color.as_ref()),
            _ => (None, None, None),
        };
        fn matches_group<T>(
            node: Option<&RawNode>,
            names: &[QName],
            value: Option<&T>,
            matches: impl FnOnce(&T, &RawNode) -> bool,
        ) -> bool {
            match value {
                Some(value) => only_choice(node, names).is_some_and(|child| matches(value, child)),
                None => !has_choice(node, names),
            }
        }
        matches_group(node, &BULLET_FONT, font, |font, child| {
            child.is(dml::BulletFont::QNAME) && font_matches(font, child)
        }) && matches_group(node, &BULLET_SIZE, size, size_matches)
            && matches_group(node, &BULLET_COLOR, color, |color, child| {
                child.is(dml::BulletColor::QNAME) && color.matches(child)
            })
            && match self {
                Self::Inherit => !has_choice(node, &BULLET_TYPE),
                Self::None => kind.is_some_and(|child| child.is(dml::NoBullet::QNAME)),
                Self::Char { char, .. } => kind.is_some_and(|child| {
                    child.is(dml::CharacterBullet::QNAME)
                        && child.attr(dml::CharacterBullet::ATTR_CHAR)
                            == Some(char.encode_utf8(&mut [0; 4]))
                }),
                Self::AutoNumber { scheme, start_at } => kind.is_some_and(|child| {
                    child.is(dml::AutoNumberedBullet::QNAME)
                        && child
                            .attr(dml::AutoNumberedBullet::ATTR_TYPE)
                            .and_then(dml::TextAutoNumberSchemeValues::parse)
                            == Some(*scheme)
                        && child
                            .attr(dml::AutoNumberedBullet::ATTR_START_AT)
                            .map_or(Some(1), int)
                            == Some(*start_at)
                }),
                Self::Picture => false,
            }
    }

    fn write(&self, lens: &mut LensMut<'_, ParagraphProperties>) -> Result<()> {
        for names in [
            BULLET_COLOR.as_slice(),
            &BULLET_SIZE,
            &BULLET_FONT,
            &BULLET_TYPE,
        ] {
            remove_choices(lens, names);
        }
        match self {
            Self::None => {
                lens.insert::<dml::NoBullet>()?;
            }
            Self::Char {
                char,
                font,
                size,
                color,
            } => {
                if let Some(color) = color {
                    color.write(&mut lens.insert::<dml::BulletColor>()?)?;
                }
                match size {
                    Some(BulletSize::FollowText) => {
                        lens.insert::<dml::BulletSizeText>()?;
                    }
                    Some(BulletSize::Percent(value)) => dml::BulletSizePercentageViewMut(
                        lens.insert::<dml::BulletSizePercentage>()?,
                    )
                    .set_val(*value)?,
                    Some(BulletSize::Points(value)) => {
                        dml::BulletSizePointsViewMut(lens.insert::<dml::BulletSizePoints>()?)
                            .set_val(*value)?
                    }
                    None => {}
                }
                if let Some(font) = font {
                    dml::BulletFontViewMut(lens.insert::<dml::BulletFont>()?)
                        .set_typeface(font.typeface()?)?;
                }
                dml::CharacterBulletViewMut(lens.insert::<dml::CharacterBullet>()?)
                    .set_char(char.encode_utf8(&mut [0; 4]))?;
            }
            Self::AutoNumber { scheme, start_at } => {
                let mut view =
                    dml::AutoNumberedBulletViewMut(lens.insert::<dml::AutoNumberedBullet>()?);
                view.set_type(*scheme)?;
                view.set_start_at(*start_at)?;
            }
            Self::Inherit => {}
            Self::Picture => return Err(Error::UnsupportedBullet),
        }
        Ok(())
    }
}

impl ParagraphMut<'_> {
    /// Replaces the direct bullet and clears any unspecified appearance.
    ///
    /// For a picture bullet, removes its image relationship when the part
    /// no longer uses it. The related media part remains in the package.
    pub fn bullet(&mut self, bullet: BulletSpec) -> Result<()> {
        bullet.validate()?;
        let paragraph = self.node()?;
        para_shape(paragraph)?;
        let props = paragraph
            .elements()
            .find(|child| child.is(ParagraphProperties::QNAME));
        if bullet.matches(props) {
            return Ok(());
        }
        let image_ids = bullet_image_ids(props);
        let mut staged = self.doc.clone();
        {
            let mut paragraph = self.loc.lens(&mut staged)?;
            bullet.write(&mut paragraph.get_or_insert::<ParagraphProperties>()?)?;
        }
        if image_ids.is_empty() {
            *self.doc = staged;
        } else {
            let EditPart::Package { package, name } = &mut self.doc else {
                return Err(Error::UnsupportedBullet);
            };
            let mut plan = RegistryPlan::new(package);
            plan.stage_part(package, name, staged)?;
            for id in image_ids {
                match plan.remove_relationship(package, Some(name), &id, &[IMAGE_REL]) {
                    Ok(())
                    | Err(ooxml_core::Error::RegistryRefused {
                        reason: RegistryRefusal::StillReferenced { .. },
                        ..
                    }) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            package.apply(plan)?;
        }
        Ok(())
    }

    /// Clears the direct bullet and its appearance.
    pub fn clear_bullet(&mut self) -> Result<()> {
        self.bullet(BulletSpec::Inherit)
    }
}

const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

fn bullet_image_ids(props: Option<&RawNode>) -> std::collections::BTreeSet<String> {
    props
        .into_iter()
        .flat_map(RawNode::elements)
        .filter(|child| child.is(dml::PictureBullet::QNAME))
        .flat_map(RawNode::elements)
        .filter(|child| child.is(dml::Blip::QNAME))
        .flat_map(|blip| {
            ["embed", "link"].into_iter().filter_map(|local| {
                blip.attr(QName::new(crate::names::R_NS, local))
                    .map(trim_xml_ws)
                    .filter(|id| !id.is_empty())
            })
        })
        .map(str::to_owned)
        .collect()
}
