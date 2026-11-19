use super::*;
use ooxml_core::EnumType;

/// An arrowhead for an explicit underline line.
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

impl UnderlineLine {
    pub(super) fn validate(&self) -> Result<()> {
        if let Self::Line(line) = self {
            if let Some(width) = line.width {
                check_range(width, 0, 20_116_800, "w")?;
            }
            if let Some(RunFill::Solid(color)) = &line.fill {
                color.validate()?;
            }
        }
        Ok(())
    }

    pub(super) fn matches(&self, child: &RawNode) -> bool {
        let Self::Line(line) = self else {
            return child.is(dml::UnderlineFollowsText::QNAME);
        };
        if !child.is(dml::Underline::QNAME) {
            return false;
        }
        let attr = |name| child.attr(QName::new("", name));
        let head = only_choice(Some(child), &[dml::HeadEnd::QNAME]);
        let tail = only_choice(Some(child), &[dml::TailEnd::QNAME]);
        attr("w").and_then(int) == line.width
            && attr("cap").and_then(dml::LineCapValues::parse) == line.cap
            && attr("cmpd").and_then(dml::CompoundLineValues::parse) == line.compound
            && only_choice(Some(child), &[dml::PresetDash::QNAME])
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

    pub(super) fn write<E: ElementSpec>(&self, parent: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::Underline: ChildOf<E>,
        dml::UnderlineFollowsText: ChildOf<E>,
    {
        let Self::Line(line) = self else {
            parent.insert::<dml::UnderlineFollowsText>()?;
            return Ok(());
        };
        let mut view = dml::UnderlineViewMut(parent.insert::<dml::Underline>()?);
        if let Some(width) = line.width {
            view.set_width(width)?;
        }
        if let Some(cap) = line.cap {
            view.set_cap_type(cap)?;
        }
        if let Some(compound) = line.compound {
            view.set_compound_line_type(compound)?;
        }
        match &line.fill {
            Some(RunFill::Solid(color)) => color.write(&mut view.0.insert::<dml::SolidFill>()?)?,
            Some(RunFill::NoFill) => {
                view.0.insert::<dml::NoFill>()?;
            }
            None => {}
        }
        if let Some(dash) = line.dash {
            dml::PresetDashViewMut(view.0.insert::<dml::PresetDash>()?).set_val(dash)?;
        }
        macro_rules! write_end {
            ($end:expr, $spec:ident, $view:ident) => {
                if let Some(end) = $end {
                    let mut end_view = dml::$view(view.0.insert::<dml::$spec>()?);
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
