use super::*;

struct RangeEdit {
    run_index: usize,
    slot_index: usize,
    text: Option<(String, std::ops::Range<usize>)>,
    properties: Option<ooxml_core::lens::SamePartDetached<RunProperties>>,
}

impl ParagraphMut<'_> {
    /// Formats a half-open range of Unicode scalar offsets. A break counts
    /// as one character and a field counts as its cached text.
    pub fn format_range(&mut self, start: usize, end: usize, format: &RunFormat) -> Result<()> {
        format.validate()?;
        if start > end {
            return Err(Error::RangeOutOfBounds);
        }
        let mut path = self.loc.body.clone();
        path.push(self.loc.child(&self.doc)?);
        let mut staged = self.doc.clone();
        let edits = plan_range(&staged, &path, start, end, format)?;
        if edits.is_empty() {
            return Ok(());
        }
        let mut paragraph = self.loc.lens(&mut staged)?;
        for edit in edits.into_iter().rev() {
            apply_range_edit(&mut paragraph, edit, format)?;
        }
        *self.doc = staged;
        Ok(())
    }
}

fn plan_range(
    doc: &XmlDocument,
    path: &[usize],
    start: usize,
    end: usize,
    format: &RunFormat,
) -> Result<Vec<RangeEdit>> {
    let paragraph = doc.lens_at::<Paragraph>(path)?;
    para_shape(paragraph.raw())?;
    let mut edits = Vec::new();
    let mut offset = 0;
    let mut run_index = 0;
    let items = paragraph
        .raw()
        .indexed_elements()
        .filter(|(_, item)| item.is(Run::QNAME) || item.is(Field::QNAME) || item.is(Break::QNAME));
    for (slot_index, (child, item)) in items.enumerate() {
        let text = item
            .elements()
            .find(|node| node.is(Text::QNAME))
            .map(RawNode::text)
            .unwrap_or_default();
        let length = if item.is(Break::QNAME) {
            1
        } else {
            text.chars().count()
        };
        let limit = offset + length;
        if item.is(Field::QNAME)
            && ((offset < start && start < limit) || (offset < end && end < limit))
        {
            return Err(Error::RangeSplitsField);
        }
        if item.is(Run::QNAME) && start.max(offset) < end.min(limit) {
            let run = doc.lens_at::<Run>(&[path, &[child]].concat())?;
            let properties = run.child::<RunProperties>();
            if format.changes(properties.as_ref().map(|node| node.raw())) {
                let mut edit = RangeEdit {
                    run_index,
                    slot_index,
                    text: None,
                    properties: None,
                };
                if start > offset || end < limit {
                    let first = start.saturating_sub(offset);
                    let last = (end - offset).min(length);
                    let byte_offset = |index| {
                        text.char_indices()
                            .nth(index)
                            .map_or(text.len(), |(byte, _)| byte)
                    };
                    let bytes = byte_offset(first)..byte_offset(last);
                    let opts = ooxml_core::lens::SamePartOptions::default()
                        .keep_extension(ooxml_core::lens::ext_uri::HYPERLINK_COLOR);
                    edit.properties = properties
                        .map(|properties| {
                            properties
                                .detach_same_part_as::<RunProperties>(&[], &opts)
                                .map_err(|err| {
                                    use ooxml_core::{DetachRefusal, Error as CoreError};
                                    match err {
                                        CoreError::NotDetachable {
                                            reason:
                                                DetachRefusal::UniqueAttribute
                                                | DetachRefusal::ExtensionList
                                                | DetachRefusal::MarkupCompatibility
                                                | DetachRefusal::AlternateContent,
                                            ..
                                        } => Error::FormatNotCopyable("a:rPr"),
                                        err => Error::Core(err),
                                    }
                                })
                        })
                        .transpose()?;
                    if let Some(copy) = &edit.properties {
                        copy.check_insert_under(doc, path, 1)?;
                    }
                    edit.text = Some((text.into_owned(), bytes));
                }
                edits.push(edit);
            }
        }
        run_index += usize::from(item.is(Run::QNAME));
        offset = limit;
    }
    if end > offset {
        return Err(Error::RangeOutOfBounds);
    }
    Ok(edits)
}

fn apply_range_edit(
    paragraph: &mut LensMut<'_, Paragraph>,
    edit: RangeEdit,
    format: &RunFormat,
) -> Result<()> {
    let Some((text, middle)) = edit.text else {
        let mut run = paragraph
            .nth_child_mut::<Run>(edit.run_index)
            .expect("planned run");
        return format.apply(&mut run);
    };
    let (left, selected, right) = (
        &text[..middle.start],
        &text[middle.clone()],
        &text[middle.end..],
    );
    {
        let mut run = paragraph
            .nth_child_mut::<Run>(edit.run_index)
            .expect("planned run");
        if left.is_empty() {
            run.get_or_insert::<Text>()?.set_text(selected)?;
            format.apply(&mut run)?;
        } else {
            run.get_or_insert::<Text>()?.set_text(left)?;
        }
    }
    let mut position = edit.slot_index + 1;
    if !left.is_empty() {
        let mut run = insert_split_run(paragraph, position, edit.properties.as_ref(), selected)?;
        format.apply(&mut run)?;
        position += 1;
    }
    if !right.is_empty() {
        insert_split_run(paragraph, position, edit.properties.as_ref(), right)?;
    }
    Ok(())
}

fn insert_split_run<'a>(
    paragraph: &'a mut LensMut<'_, Paragraph>,
    position: usize,
    props: Option<&ooxml_core::lens::SamePartDetached<RunProperties>>,
    text: &str,
) -> Result<LensMut<'a, Run>> {
    let mut run = paragraph.insert_by::<Run>(|index, _| index == position)?;
    if let Some(props) = props {
        run.insert_same_part(props)?;
    }
    run.insert::<Text>()?.set_text(text)?;
    Ok(run)
}
