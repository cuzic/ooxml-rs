use std::collections::BTreeSet;

use ooxml_core::{
    ChildOf, Content, ElementSpec, NsDecl, QName, RawNode, RegistryPlan, Slot, XmlDocument,
    name::MC_NS,
};
use ooxml_schema::wml::{
    Body, Checked, CheckedState, Comment, DataBinding, Endnote, Footer, Footnote, Header, ListItem,
    Lock, NS_W, Paragraph, Run, SdtAlias, SdtBlock, SdtContentBlock, SdtContentCheckBox,
    SdtContentDate, SdtContentDropDownList, SdtContentRun, SdtContentText, SdtId as IdElement,
    SdtProperties, SdtRun, TableCell, Tag, UncheckedState,
};

use super::{Binding, SdtId, W14, child, w14};
use crate::bookmarks::{Boundary, split_boundary};
use crate::document::node_at;
use crate::names::{SDT, VAL, w};
use crate::{Document, Error, ParaRef, Result, TextRange};

const W: &[NsDecl<'_>] = &[NsDecl::new("w", NS_W)];
const MC: &str = "mc";
const CHECKBOX_NS: &[NsDecl<'_>] = &[
    NsDecl::new("w", NS_W),
    NsDecl::new("w14", W14),
    NsDecl::new(MC, MC_NS),
];

/// Existing text or paragraphs to wrap.
#[derive(Clone, Copy, Debug)]
pub enum WrapTarget {
    /// Inline range in a single paragraph.
    Inline(TextRange),
    /// Inclusive range of sibling paragraphs.
    Paragraphs {
        /// First paragraph.
        first: ParaRef,
        /// Last paragraph.
        last: ParaRef,
    },
}

/// Type and initial settings of the new control.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NewControlKind {
    /// Plain text with optional line breaks.
    PlainText {
        /// Whether newline characters are allowed.
        multi_line: bool,
    },
    /// Rich text.
    #[default]
    RichText,
    /// Date picker.
    Date,
    /// Checkbox with the specified initial state.
    Checkbox {
        /// Initial checked state.
        checked: bool,
    },
    /// List of (value, display text) pairs.
    DropDown(Vec<(String, String)>),
}

/// Editing lock written to the control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlLock {
    /// Allow value edits and removal.
    #[default]
    Unlocked,
    /// Prevent removal of the control.
    Control,
    /// Prevent edits to the contents.
    Content,
    /// Prevent both removal and editing.
    Both,
}

/// Properties of a newly wrapped content control.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlSpec {
    /// Kind of control.
    pub kind: NewControlKind,
    /// Optional programmatic tag.
    pub tag: Option<String>,
    /// Optional user-facing title.
    pub alias: Option<String>,
    /// Editing and removal lock.
    pub lock: ControlLock,
    /// Optional existing XML store element.
    pub binding: Option<Binding>,
}

impl Document {
    /// Wraps an inline range or adjacent paragraphs in a content control.
    pub fn wrap_in_control(&mut self, target: WrapTarget, spec: ControlSpec) -> Result<SdtId> {
        self.wrap_with_ids(target, spec, || {
            let mut bytes = [0; 4];
            getrandom::fill(&mut bytes).map_err(|_| Error::SdtIdRandomness)?;
            Ok(i32::from_ne_bytes(bytes))
        })
    }

    fn wrap_with_ids(
        &mut self,
        target: WrapTarget,
        spec: ControlSpec,
        source: impl FnMut() -> Result<i32>,
    ) -> Result<SdtId> {
        self.content_control_gate(false)?;
        let ids = self
            .stories()
            .flat_map(|story| super::controls(story.root))
            .flat_map(|(_, node)| node.elements().filter(|pr| pr.is(w("sdtPr"))))
            .flat_map(RawNode::elements)
            .filter(|node| node.is(w("id")))
            .filter_map(|node| node.attr(VAL)?.parse().ok())
            .collect();
        let id = allocate(ids, source)?;
        let (first, last) = match target {
            WrapTarget::Inline(range) => (range.first.para(), range.last.para()),
            WrapTarget::Paragraphs { first, last } => (first, last),
        };
        let story = first.story();
        if story != last.story() || first.index() > last.index() {
            return Err(Error::InvalidTextRange);
        }
        let part = self
            .stories
            .get(story.0)
            .ok_or(Error::InvalidTextRange)?
            .id
            .part
            .clone();
        let mut xml = self.pkg.parsed_part(&part).expect("parsed story").clone();
        let text = match target {
            WrapTarget::Inline(range) => self.wrap_inline(&mut xml, range, &spec, id)?,
            WrapTarget::Paragraphs { first, last } => {
                self.wrap_blocks(&mut xml, first, last, &spec, id)?
            }
        };
        if let Some(binding) = &spec.binding {
            let target = binding.target()?;
            let (_, store) = target.store(self)?;
            if target.value(&store)? != text {
                return Err(Error::BindingMismatch);
            }
        }
        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, &part, xml)?;
        self.pkg.apply(plan)?;
        self.refresh_control_story(story);
        Ok(id)
    }

    fn wrap_inline(
        &self,
        xml: &mut XmlDocument,
        range: TextRange,
        spec: &ControlSpec,
        id: SdtId,
    ) -> Result<String> {
        let para = range.first.para();
        if para != range.last.para() || range.first.index() > range.last.index() {
            return Err(Error::InvalidTextRange);
        }
        let paragraph = self.paragraph_at(para).ok_or(Error::InvalidTextRange)?;
        let path = paragraph.data.index.path(paragraph.info.frame);
        let mut items = Vec::new();
        let mut text = String::new();
        let mut start = None;
        let mut end = None;
        for index in range.first.index()..=range.last.index() {
            let run = paragraph.run(index).ok_or(Error::InvalidTextRange)?;
            run.check_settable()?;
            let run_path = paragraph.data.index.path(run.info.frame);
            let (&item, parent) = run_path.split_last().ok_or(Error::InvalidTextRange)?;
            if parent != path {
                return Err(Error::InvalidTextRange);
            }
            let value = run.text();
            let from = if index == range.first.index() {
                range.start_byte.unwrap_or(0)
            } else {
                0
            };
            let to = if index == range.last.index() {
                range.end_byte.unwrap_or(value.len())
            } else {
                value.len()
            };
            text.push_str(value.get(from..to).ok_or(Error::InvalidTextRange)?);
            if index == range.first.index() {
                if from == value.len() && range.start_byte.is_some() {
                    return Err(Error::InvalidTextRange);
                }
                start = Some(if from == 0 {
                    Boundary::Between(item)
                } else {
                    Boundary::Inside {
                        child: item,
                        byte: from,
                    }
                });
            }
            if index == range.last.index() {
                if to == 0 && range.end_byte.is_some() {
                    return Err(Error::InvalidTextRange);
                }
                end = Some(if to == value.len() {
                    Boundary::Between(item + 1)
                } else {
                    Boundary::Inside {
                        child: item,
                        byte: to,
                    }
                });
            }
            items.push(item);
        }
        let parent = node_at(xml.root(), &path);
        if !contiguous(parent, &items) || text.is_empty() {
            return Err(Error::InvalidTextRange);
        }
        let mut end = split_boundary::<Paragraph>(xml, &path, end.ok_or(Error::InvalidTextRange)?)?;
        let start_boundary = start.ok_or(Error::InvalidTextRange)?;
        if matches!(start_boundary, Boundary::Inside { .. }) {
            end += 1;
        }
        let start = split_boundary::<Paragraph>(xml, &path, start_boundary)?;
        let items: Vec<_> = node_at(xml.root(), &path)
            .indexed_elements()
            .filter(|(index, _)| (start..end).contains(index))
            .map(|(i, _)| i)
            .collect();

        let mut parent = xml.lens_mut_at::<Paragraph>(&path)?;
        let mut sdt = parent.insert_at_with_ns::<SdtRun>(start, declarations(spec))?;
        let sdt_path = sdt.path().to_vec();
        sdt.insert::<SdtProperties>()?;
        let content = sdt.insert::<SdtContentRun>()?.path().to_vec();
        properties(xml, &sdt_path, spec, id)?;
        for item in items.into_iter().rev() {
            let path = [path.as_slice(), &[item + 1]].concat();
            let source = xml.lens_at::<Run>(&path)?.move_source();
            xml.lens_mut_at::<SdtContentRun>(&content)?
                .move_in_by::<Run>(&source, |_, _| true)?;
        }
        Ok(text)
    }

    fn wrap_blocks(
        &self,
        xml: &mut XmlDocument,
        first: ParaRef,
        last: ParaRef,
        spec: &ControlSpec,
        id: SdtId,
    ) -> Result<String> {
        let mut items = Vec::new();
        let mut texts = Vec::new();
        let first_paragraph = self.paragraph_at(first).ok_or(Error::InvalidTextRange)?;
        let first_path = first_paragraph.data.index.path(first_paragraph.info.frame);
        let (_, parent_path) = first_path.split_last().ok_or(Error::InvalidTextRange)?;
        for index in first.index()..=last.index() {
            let paragraph = self
                .paragraph_at(ParaRef::new(first.story(), index))
                .ok_or(Error::InvalidTextRange)?;
            let path = paragraph.data.index.path(paragraph.info.frame);
            let (&item, parent) = path.split_last().ok_or(Error::InvalidTextRange)?;
            if parent != parent_path
                || (0..path.len()).any(|depth| node_at(xml.root(), &path[..depth]).is(SDT))
            {
                return Err(Error::InvalidTextRange);
            }
            let node = node_at(xml.root(), &path);
            if child(node, w("pPr")).is_some_and(|pr| child(pr, w("sectPr")).is_some()) {
                return Err(Error::InvalidTextRange);
            }
            self.check_control_runs(first.story(), &path)?;
            items.push(item);
            texts.push(paragraph.text());
        }
        let parent = node_at(xml.root(), parent_path);
        if !contiguous(parent, &items) {
            return Err(Error::InvalidTextRange);
        }
        let content = insert_block(xml, parent_path, items[0], spec, id)?;
        for item in items.into_iter().rev() {
            let path = [parent_path, &[item + 1]].concat();
            let source = xml.lens_at::<Paragraph>(&path)?.move_source();
            xml.lens_mut_at::<SdtContentBlock>(&content)?
                .move_in_by::<Paragraph>(&source, |_, _| true)?;
        }
        Ok(texts.join(
            "
",
        ))
    }
}

fn allocate(used: BTreeSet<i32>, mut source: impl FnMut() -> Result<i32>) -> Result<SdtId> {
    loop {
        let id = source()?;
        if id != 0 && !used.contains(&id) {
            return Ok(SdtId(id));
        }
    }
}

fn contiguous(parent: &ooxml_core::RawNode, items: &[usize]) -> bool {
    let Some((&first, rest)) = items.split_first() else {
        return false;
    };
    let last = *rest.last().unwrap_or(&first);
    parent
        .indexed_elements()
        .filter(|(i, _)| (first..=last).contains(i))
        .map(|(i, _)| i)
        .eq(items.iter().copied())
}

fn insert_block(
    xml: &mut XmlDocument,
    path: &[usize],
    index: usize,
    spec: &ControlSpec,
    id: SdtId,
) -> Result<Vec<usize>> {
    let parent = node_at(xml.root(), path);
    macro_rules! insert {
        ($($ty:ty),+ $(,)?) => {$(
            if parent.is(<$ty>::QNAME) {
                let mut parent = xml.lens_mut_at::<$ty>(path)?;
                let mut sdt = parent.insert_at_with_ns::<SdtBlock>(index, declarations(spec))?;
                let sdt_path = sdt.path().to_vec();
                sdt.insert::<SdtProperties>()?;
                let content = sdt.insert::<SdtContentBlock>()?.path().to_vec();
                properties(xml, &sdt_path, spec, id)?;
                return Ok(content);
            }
        )+};
    }
    insert!(Body, TableCell, Header, Footer, Footnote, Endnote, Comment);
    Err(Error::InvalidTextRange)
}

fn declarations(spec: &ControlSpec) -> &'static [NsDecl<'static>] {
    if matches!(spec.kind, NewControlKind::Checkbox { .. }) {
        CHECKBOX_NS
    } else {
        W
    }
}

// The SDK and Transitional XSD order the properties differently. This
// private projection creates only the subset used here, in XSD order.
enum NewProperties {}

const fn singleton(names: &'static [QName]) -> Slot {
    Slot {
        names,
        wildcard: None,
        max_each: Some(1),
        max_total: Some(1),
    }
}

impl ElementSpec for NewProperties {
    const QNAME: QName = SdtProperties::QNAME;
    const SDK_NAME: &'static str = SdtProperties::SDK_NAME;
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: Content = Content::Slots(&[
        singleton(&[SdtAlias::QNAME]),
        singleton(&[Tag::QNAME]),
        singleton(&[IdElement::QNAME]),
        singleton(&[Lock::QNAME]),
        singleton(&[DataBinding::QNAME]),
        singleton(&[
            SdtContentText::QNAME,
            SdtContentDate::QNAME,
            SdtContentDropDownList::QNAME,
            SdtContentCheckBox::QNAME,
        ]),
    ]);
}

impl ChildOf<NewProperties> for SdtAlias {}
impl ChildOf<NewProperties> for Tag {}
impl ChildOf<NewProperties> for IdElement {}
impl ChildOf<NewProperties> for Lock {}
impl ChildOf<NewProperties> for DataBinding {}
impl ChildOf<NewProperties> for SdtContentText {}
impl ChildOf<NewProperties> for SdtContentDate {}
impl ChildOf<NewProperties> for SdtContentDropDownList {}
impl ChildOf<NewProperties> for SdtContentCheckBox {}

// The SDK SDT base class omits attributes. This projection is used only
// to write the Markup Compatibility attribute on a newly inserted SDT.
enum NewControlAttributes {}

impl ElementSpec for NewControlAttributes {
    const QNAME: QName = SdtRun::QNAME;
    const SDK_NAME: &'static str = SdtRun::SDK_NAME;
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: Content = Content::Empty;
}

fn properties(xml: &mut XmlDocument, path: &[usize], spec: &ControlSpec, id: SdtId) -> Result<()> {
    let pr_index = node_at(xml.root(), path)
        .indexed_elements()
        .find(|(_, pr)| pr.is(SdtProperties::QNAME))
        .expect("new properties")
        .0;
    let pr_path = [path, &[pr_index]].concat();
    let mut pr = xml.lens_mut_at::<NewProperties>(&pr_path)?;
    if let Some(alias) = &spec.alias {
        pr.insert::<SdtAlias>()?.set_attr(VAL, alias)?;
    }
    if let Some(tag) = &spec.tag {
        pr.insert::<Tag>()?.set_attr(VAL, tag)?;
    }
    pr.insert::<IdElement>()?
        .set_attr(VAL, id.0.format_into(&mut core::fmt::NumBuffer::new()))?;
    if let Some(lock) = match spec.lock {
        ControlLock::Unlocked => None,
        ControlLock::Control => Some("sdtLocked"),
        ControlLock::Content => Some("contentLocked"),
        ControlLock::Both => Some("sdtContentLocked"),
    } {
        pr.insert::<Lock>()?.set_attr(VAL, lock)?;
    }
    if let Some(binding) = &spec.binding {
        let mut node = pr.insert::<DataBinding>()?;
        node.set_attr(w("storeItemID"), &binding.store_item_id)?;
        node.set_attr(w("prefixMappings"), &binding.prefix_mappings)?;
        node.set_attr(w("xpath"), &binding.xpath)?;
    }
    match &spec.kind {
        NewControlKind::PlainText { multi_line } => {
            let mut node = pr.insert::<SdtContentText>()?;
            if *multi_line {
                node.set_attr(w("multiLine"), "1")?;
            }
        }
        NewControlKind::RichText => {}
        NewControlKind::Date => {
            pr.insert::<SdtContentDate>()?;
        }
        NewControlKind::Checkbox { checked } => {
            let mut box_node = pr.insert::<SdtContentCheckBox>()?;
            box_node
                .insert::<Checked>()?
                .set_attr(w14("val"), if *checked { "1" } else { "0" })?;
            let mut on = box_node.insert::<CheckedState>()?;
            on.set_attr(w14("val"), "2612")?;
            on.set_attr(w14("font"), "MS Gothic")?;
            let mut off = box_node.insert::<UncheckedState>()?;
            off.set_attr(w14("val"), "2610")?;
            off.set_attr(w14("font"), "MS Gothic")?;
            let prefix = box_node
                .node()
                .name()
                .prefix()
                .ok_or(Error::UnsupportedControl)?
                .to_owned();
            xml.lens_mut_at::<NewControlAttributes>(path)?
                .set_attr(QName::new(MC_NS, "Ignorable"), &prefix)?;
        }
        NewControlKind::DropDown(items) => {
            let mut list = pr.insert::<SdtContentDropDownList>()?;
            for (value, display) in items {
                let mut node = list.insert::<ListItem>()?;
                node.set_attr(w("value"), value)?;
                node.set_attr(w("displayText"), display)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_zero_and_existing_ids() {
        let mut ids = [0, 27, -3, -4].into_iter();
        assert_eq!(
            allocate(BTreeSet::from([27, -3]), || Ok(ids.next().unwrap())).unwrap(),
            SdtId(-4)
        );
    }
}
