use super::*;
use ooxml_core::EnumType;

/// An arrowhead for an explicit line or underline line.
///
/// When used with [`crate::ShapeMut::set_line`], each `None` field leaves the
/// corresponding attribute on an existing arrowhead unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineEnd {
    /// Arrowhead type.
    pub kind: Option<dml::LineEndValues>,
    /// Arrowhead width.
    pub width: Option<dml::LineEndWidthValues>,
    /// Arrowhead length.
    pub length: Option<dml::LineEndLengthValues>,
}

/// Explicit underline line properties.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineSpec {
    /// Line width in EMU.
    pub width: Option<i32>,
    /// Line fill.
    pub fill: Option<RunFill>,
    /// Preset dash pattern.
    pub dash: Option<dml::PresetLineDashValues>,
    /// Line cap.
    pub cap: Option<dml::LineCapValues>,
    /// Compound line type.
    pub compound: Option<dml::CompoundLineValues>,
    /// Leading arrowhead.
    pub head: Option<LineEnd>,
    /// Trailing arrowhead.
    pub tail: Option<LineEnd>,
}

/// The underline line choice.
#[derive(Clone, Debug, PartialEq)]
pub enum UnderlineLine {
    /// Follow the text's line properties.
    FollowText,
    /// Explicit line properties.
    Line(LineSpec),
}

/// The underline fill choice.
#[derive(Clone, Debug, PartialEq)]
pub enum UnderlineFill {
    /// Follow the text's fill.
    FollowText,
    /// Direct solid colour.
    Solid(ColorSpec),
}

pub(super) const LINE: [QName; 2] = [dml::Underline::QNAME, dml::UnderlineFollowsText::QNAME];
pub(super) const FILL_CHOICE: [QName; 2] =
    [dml::UnderlineFill::QNAME, dml::UnderlineFillText::QNAME];

impl LineEnd {
    fn matches(&self, node: &RawNode) -> bool {
        let attr = |name| node.attr(QName::new("", name));
        attr("type").and_then(dml::LineEndValues::parse) == self.kind
            && attr("w").and_then(dml::LineEndWidthValues::parse) == self.width
            && attr("len").and_then(dml::LineEndLengthValues::parse) == self.length
    }
}

impl LineSpec {
    pub(in crate::write) fn validate(&self) -> Result<()> {
        if let Some(width) = self.width {
            check_range(width, 0, 20_116_800, "w")?;
        }
        if let Some(RunFill::Solid(color)) = &self.fill {
            color.validate()?;
        }
        Ok(())
    }

    pub(in crate::write) fn matches(&self, child: &RawNode) -> bool {
        let line = self;
        let attr = |name| child.attr(QName::new("", name));
        let head = only_choice(Some(child), &[dml::HeadEnd::QNAME]);
        let tail = only_choice(Some(child), &[dml::TailEnd::QNAME]);
        let dash = only_choice(Some(child), &[dml::PresetDash::QNAME]);
        let expected_children = [
            line.fill.is_some(),
            line.dash.is_some(),
            line.head.is_some(),
            line.tail.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        let expected_attrs = [
            line.width.is_some(),
            line.cap.is_some(),
            line.compound.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        child.elements().count() == expected_children
            && child.attrs().len() == expected_attrs
            && attr("w").and_then(int) == line.width
            && attr("cap").and_then(dml::LineCapValues::parse) == line.cap
            && attr("cmpd").and_then(dml::CompoundLineValues::parse) == line.compound
            && dash.is_some() == line.dash.is_some()
            && dash
                .and_then(|dash| dash.attr(dml::PresetDash::ATTR_VAL))
                .and_then(dml::PresetLineDashValues::parse)
                == line.dash
            && match (&line.fill, only_choice(Some(child), &FILL)) {
                (Some(fill), Some(node)) => fill_matches(fill, node),
                (None, None) => true,
                _ => false,
            }
            && head.is_some() == line.head.is_some()
            && tail.is_some() == line.tail.is_some()
            && line
                .head
                .zip(head)
                .is_none_or(|(end, node)| end.matches(node))
            && line
                .tail
                .zip(tail)
                .is_none_or(|(end, node)| end.matches(node))
    }

    pub(in crate::write) fn update_outline(
        &self,
        node: &mut LensMut<'_, dml::Outline>,
    ) -> Result<()> {
        let mut update = self.clone();
        let ooxml_core::Content::Slots(slots) = dml::Outline::CONTENT else {
            unreachable!("outline has ordered slots")
        };
        if update.fill.is_some()
            || update.dash.is_some()
            || update.head.is_some()
            || update.tail.is_some()
        {
            let mut next_slot = 0;
            for child in node.node().elements() {
                let Some(index) = slots[next_slot..]
                    .iter()
                    .position(|slot| slot.names.iter().any(|&name| child.is(name)))
                else {
                    return Err(Error::UnsupportedContent(
                        "ambiguous outline children".into(),
                    ));
                };
                next_slot += index + 1;
            }
        }
        let fill_slot = slots
            .iter()
            .find(|slot| slot.names.contains(&dml::SolidFill::QNAME))
            .expect("outline fill slot")
            .names;
        if let Some(fill) = &update.fill {
            if only_choice(Some(node.node()), fill_slot).is_some_and(|old| fill_matches(fill, old))
            {
                update.fill = None;
            } else {
                remove_choices(node, fill_slot);
            }
        }
        if update.dash.is_some() {
            remove_choices(node, &[QName::new(dml::PresetDash::QNAME.ns, "custDash")]);
        }
        node.preserve_attribute_spelling();
        update.write(node)
    }

    pub(in crate::write) fn write<E: ElementSpec>(&self, node: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::SolidFill: ChildOf<E>,
        dml::NoFill: ChildOf<E>,
        dml::PresetDash: ChildOf<E>,
        dml::HeadEnd: ChildOf<E>,
        dml::TailEnd: ChildOf<E>,
    {
        let line = self;
        if let Some(width) = line.width
            && node.node().attr(QName::new("", "w")).and_then(int) != Some(width)
        {
            ooxml_core::value::with_i32(width, |value| node.set_attr(QName::new("", "w"), value))?;
        }
        if let Some(cap) = line.cap {
            node.set_attr(QName::new("", "cap"), cap.as_str())?;
        }
        if let Some(compound) = line.compound {
            node.set_attr(QName::new("", "cmpd"), compound.as_str())?;
        }
        match &line.fill {
            Some(RunFill::Solid(color)) => color.write(&mut node.insert::<dml::SolidFill>()?)?,
            Some(RunFill::NoFill) => {
                node.insert::<dml::NoFill>()?;
            }
            None => {}
        }
        if let Some(dash) = line.dash {
            let mut dash_node = node.get_or_insert::<dml::PresetDash>()?;
            dash_node.preserve_attribute_spelling();
            dml::PresetDashViewMut(dash_node).set_val(dash)?;
        }
        macro_rules! write_end {
            ($end:expr, $spec:ident, $view:ident) => {
                if let Some(end) = $end {
                    let mut end_node = node.get_or_insert::<dml::$spec>()?;
                    end_node.preserve_attribute_spelling();
                    let mut end_view = dml::$view(end_node);
                    if let Some(kind) = end.kind {
                        end_view.set_type(kind)?;
                    }
                    if let Some(width) = end.width {
                        end_view.set_width(width)?;
                    }
                    if let Some(length) = end.length {
                        end_view.set_length(length)?;
                    }
                }
            };
        }
        write_end!(line.head, HeadEnd, HeadEndViewMut);
        write_end!(line.tail, TailEnd, TailEndViewMut);
        Ok(())
    }
}

impl UnderlineLine {
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Line(line) => line.validate(),
            Self::FollowText => Ok(()),
        }
    }

    pub(super) fn matches(&self, child: &RawNode) -> bool {
        match self {
            Self::Line(line) => child.is(dml::Underline::QNAME) && line.matches(child),
            Self::FollowText => child.is(dml::UnderlineFollowsText::QNAME),
        }
    }

    pub(super) fn write<E: ElementSpec>(&self, parent: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::Underline: ChildOf<E>,
        dml::UnderlineFollowsText: ChildOf<E>,
    {
        match self {
            Self::Line(line) => line.write(&mut parent.insert::<dml::Underline>()?),
            Self::FollowText => {
                parent.insert::<dml::UnderlineFollowsText>()?;
                Ok(())
            }
        }
    }
}

impl UnderlineFill {
    pub(super) fn matches(&self, child: &RawNode) -> bool {
        match self {
            Self::FollowText => child.is(dml::UnderlineFillText::QNAME),
            Self::Solid(color) => {
                child.is(dml::UnderlineFill::QNAME)
                    && only_choice(Some(child), &FILL)
                        .is_some_and(|node| node.is(dml::SolidFill::QNAME) && color.matches(node))
            }
        }
    }

    pub(super) fn write<E: ElementSpec>(&self, parent: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::UnderlineFill: ChildOf<E>,
        dml::UnderlineFillText: ChildOf<E>,
    {
        match self {
            Self::FollowText => {
                parent.insert::<dml::UnderlineFillText>()?;
            }
            Self::Solid(color) => color.write(
                &mut parent
                    .insert::<dml::UnderlineFill>()?
                    .insert::<dml::SolidFill>()?,
            )?,
        }
        Ok(())
    }
}

impl RunMut<'_> {
    /// Writes the underline line choice.
    pub fn underline_line(&mut self, line: UnderlineLine) -> Result<()> {
        self.format(&RunFormat {
            underline_line: Some(FormatChange::Set(line)),
            ..RunFormat::default()
        })
    }

    /// Clears the underline line choice.
    pub fn clear_underline_line(&mut self) -> Result<()> {
        self.format(&RunFormat {
            underline_line: Some(FormatChange::Clear),
            ..RunFormat::default()
        })
    }

    /// Writes the underline fill choice.
    pub fn underline_fill(&mut self, fill: UnderlineFill) -> Result<()> {
        self.format(&RunFormat {
            underline_fill: Some(FormatChange::Set(fill)),
            ..RunFormat::default()
        })
    }

    /// Clears the underline fill choice.
    pub fn clear_underline_fill(&mut self) -> Result<()> {
        self.format(&RunFormat {
            underline_fill: Some(FormatChange::Clear),
            ..RunFormat::default()
        })
    }
}
