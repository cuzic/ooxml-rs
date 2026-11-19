use super::*;
use dml::{BodyProperties, NoAutoFit, NormalAutoFit, NormalAutoFitViewMut, ShapeAutoFit};

/// The explicit autofit choice for a text body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoFitMode {
    /// Disable autofit.
    None,
    /// Scale text to fit the shape.
    Normal,
    /// Resize the shape to fit the text.
    Shape,
}

const AUTOFIT: [QName; 3] = [NoAutoFit::QNAME, NormalAutoFit::QNAME, ShapeAutoFit::QNAME];

impl TextBodyMut<'_> {
    /// Sets the direct autofit choice. An existing normal-autofit scale
    /// stays cached until an explicit reset or until PowerPoint refits.
    pub fn autofit(&mut self, mode: AutoFitMode) -> Result<()> {
        body_paragraphs(self.node())?;
        let mut staged = self.doc.clone();
        let body = body_lens(&mut staged, &self.path)?;
        with_body!(body, lens => set_autofit(lens, mode))?;
        *self.doc = staged;
        Ok(())
    }

    /// Clears the direct autofit choice so the body can inherit it.
    pub fn clear_autofit(&mut self) -> Result<()> {
        body_paragraphs(self.node())?;
        let body = body_lens(&mut self.doc, &self.path)?;
        with_body!(body, lens => clear_autofit(lens))
    }

    /// Drops the cached font scale and line spacing reduction.
    pub fn reset_autofit_scale(&mut self) -> Result<()> {
        body_paragraphs(self.node())?;
        let body = body_lens(&mut self.doc, &self.path)?;
        with_body!(body, lens => reset_autofit_scale(lens))
    }
}

fn set_autofit<E: ElementSpec>(mut lens: LensMut<'_, E>, mode: AutoFitMode) -> Result<()>
where
    BodyProperties: ChildOf<E>,
{
    let qname = match mode {
        AutoFitMode::None => NoAutoFit::QNAME,
        AutoFitMode::Normal => NormalAutoFit::QNAME,
        AutoFitMode::Shape => ShapeAutoFit::QNAME,
    };
    let mut properties = lens.get_or_insert::<BodyProperties>()?;
    let mut choices = properties
        .node()
        .elements()
        .filter(|child| AUTOFIT.iter().any(|&q| child.is(q)));
    if choices.next().is_some_and(|child| child.is(qname)) && choices.next().is_none() {
        return Ok(());
    }
    drop(choices);
    remove_choices(&mut properties, &AUTOFIT);
    match mode {
        AutoFitMode::None => drop(properties.insert::<NoAutoFit>()?),
        AutoFitMode::Normal => drop(properties.insert::<NormalAutoFit>()?),
        AutoFitMode::Shape => drop(properties.insert::<ShapeAutoFit>()?),
    }
    Ok(())
}

fn clear_autofit<E: ElementSpec>(mut lens: LensMut<'_, E>) -> Result<()>
where
    BodyProperties: ChildOf<E>,
{
    if let Some(mut properties) = lens.child_mut::<BodyProperties>() {
        remove_choices(&mut properties, &AUTOFIT);
    }
    Ok(())
}

fn reset_autofit_scale<E: ElementSpec>(mut lens: LensMut<'_, E>) -> Result<()>
where
    BodyProperties: ChildOf<E>,
{
    if let Some(mut props) = lens.child_mut::<BodyProperties>()
        && let Some(normal) = props.child_mut::<NormalAutoFit>()
    {
        let mut normal = NormalAutoFitViewMut(normal);
        normal.remove_font_scale()?;
        normal.remove_line_space_reduction()?;
    }
    Ok(())
}
