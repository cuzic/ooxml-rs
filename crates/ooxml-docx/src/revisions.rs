//! Tracked edits and revision metadata (ADR-0015 §5).

use ooxml_core::lens::NsDecl;
use ooxml_core::name::XML_SPACE;
use ooxml_core::{ChildOf, ElementSpec, LensMut};
use ooxml_core::{QName, RawNode, RegistryPlan, XmlDocument};
use ooxml_schema::wml::{
    Deleted, DeletedFieldCode, DeletedRun, DeletedText, FieldCode, Hyperlink, Inserted,
    InsertedRun, NS_W, Paragraph as P, ParagraphMarkRunProperties, ParagraphProperties,
    ParagraphPropertiesChange, ParagraphPropertiesExtended, PreviousRunProperties, Run as R,
    RunProperties, RunPropertiesChange, SectionProperties, Text,
};

use crate::bookmarks::{Boundary, annotation_id, split_boundary};
use crate::comments::{LocalDateTime, dates};
use crate::document::{node_at, write_pieces};
use crate::format::{apply_ppr, apply_rpr, plan_paragraph, plan_run};
use crate::names::{VAL, descendants};
use crate::story::{ParaRef, RunRef, StoryIndex};
use crate::structure::{Container, container_at, next_slot_sibling, on_container};
use crate::text::{Piece, current_pieces, lacks_needed_preserve, map_text};
use crate::{
    Document, Error, Operation, ParagraphFormat, ParagraphSpec, Place, Result, Run, RunContext,
    RunFormat, RunPosition, TextRange,
};

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}

pub(crate) const REVISION_NS: &[NsDecl<'_>] = &[NsDecl {
    prefix: "w",
    uri: NS_W,
}];

/// The author written on each revision created by a session.
#[derive(Clone, Debug)]
pub struct Author(String);

impl Author {
    /// Creates an author name. Starting a session validates the name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// The kind of revision recorded on a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RevisionKind {
    Insertion,
    Deletion,
    MoveFrom,
    MoveTo,
    Formatting,
}

/// Metadata from an enclosing revision or the run's own formatting change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revision<'a> {
    pub kind: RevisionKind,
    pub id: Option<&'a str>,
    pub author: Option<&'a str>,
    pub date: Option<&'a str>,
}

fn kind(node: &RawNode) -> Option<RevisionKind> {
    if node.name().ns() != NS_W {
        return None;
    }
    match node.name().local() {
        "ins" => Some(RevisionKind::Insertion),
        "del" => Some(RevisionKind::Deletion),
        "moveFrom" => Some(RevisionKind::MoveFrom),
        "moveTo" => Some(RevisionKind::MoveTo),
        "rPrChange" => Some(RevisionKind::Formatting),
        _ => None,
    }
}

pub(crate) fn range_marker(node: &RawNode) -> Option<(&str, &str, bool)> {
    if node.name().ns() != NS_W {
        return None;
    }
    let local = node.name().local();
    let (kind, start) = if let Some(kind) = local.strip_suffix("RangeStart") {
        (kind, true)
    } else {
        (local.strip_suffix("RangeEnd")?, false)
    };
    matches!(
        kind,
        "moveFrom"
            | "moveTo"
            | "customXmlIns"
            | "customXmlDel"
            | "customXmlMoveFrom"
            | "customXmlMoveTo"
    )
    .then(|| (kind, node.attr(w("id")).unwrap_or_default(), start))
}

fn context(node: &RawNode) -> Option<RunContext> {
    kind(node)
        .map(|kind| match kind {
            RevisionKind::Insertion => RunContext::Inserted,
            RevisionKind::Deletion => RunContext::Deleted,
            RevisionKind::MoveFrom => RunContext::MovedFrom,
            RevisionKind::MoveTo => RunContext::MovedTo,
            RevisionKind::Formatting => RunContext::Direct,
        })
        .or_else(|| {
            (node.name().ns() == NS_W
                && matches!(
                    node.name().local(),
                    "pPrChange"
                        | "numberingChange"
                        | "sectPrChange"
                        | "tblPrChange"
                        | "tblPrExChange"
                        | "trPrChange"
                        | "tcPrChange"
                        | "tblGridChange"
                        | "cellIns"
                        | "cellDel"
                        | "cellMerge"
                )
                || range_marker(node).is_some())
            .then_some(RunContext::Direct)
        })
}

fn check_nodes<'a>(nodes: impl Iterator<Item = &'a RawNode>) -> Result<()> {
    nodes
        .filter_map(context)
        .next()
        .map_or(Ok(()), |context| Err(Error::InsideRevision { context }))
}

fn property_revisions(node: &RawNode) -> impl Iterator<Item = &RawNode> {
    node.elements()
        .filter(|n| {
            n.name().ns() == NS_W
                && matches!(
                    n.name().local(),
                    "pPr" | "rPr" | "tblPr" | "tblPrEx" | "tblGrid" | "trPr" | "tcPr"
                )
        })
        .flat_map(descendants)
}

pub(crate) fn check_path(root: &RawNode, path: &[usize]) -> Result<()> {
    let mut node = root;
    for &item in path {
        check_nodes(std::iter::once(node).chain(property_revisions(node)))?;
        node = node.children()[item].as_element().expect("indexed element");
    }
    check_nodes(std::iter::once(node).chain(property_revisions(node)))
}

pub(crate) fn check_run_revision(run: &Run<'_>) -> Result<()> {
    let p = &run.paragraph;
    if run.info.in_revision_range {
        return Err(Error::InsideRevision {
            context: RunContext::Direct,
        });
    }
    if let Some(wrapper) = run.wrappers().into_iter().find(|w| w.is_revision()) {
        return Err(Error::InsideRevision {
            context: wrapper.context(),
        });
    }
    check_path(
        p.doc.story_root_node(p.para_ref().story()),
        &p.data.index.path(run.info.frame),
    )
}

impl<'a> Run<'a> {
    /// The nearest enclosing content revision, or this run's formatting revision.
    pub fn revision(&self) -> Option<Revision<'a>> {
        let p = &self.paragraph;
        let path = p.data.index.path(self.info.frame);
        let root = p.doc.story_root_node(p.para_ref().story());
        let enclosing = (0..path.len())
            .rev()
            .map(|i| node_at(root, &path[..i]))
            .find(|n| kind(n).is_some());
        let node = enclosing.or_else(|| {
            self.raw()
                .elements()
                .find(|n| n.is(RunProperties::QNAME))?
                .elements()
                .find(|n| n.is(RunPropertiesChange::QNAME))
        })?;
        Some(Revision {
            kind: kind(node)?,
            id: node.attr(w("id")),
            author: node.attr(w("author")),
            date: node.attr(w("date")),
        })
    }
}

/// A scoped tracked editing session. Revision settings are preserved.
/// Editing the scope of an earlier revision, including one generated by
/// this session, is refused. When an edit inserts or splits runs or paragraphs,
/// indices after the edit point can shift.
#[derive(Debug)]
pub struct TrackedChanges<'a> {
    pub(crate) doc: &'a mut Document,
    author: Author,
    date: Option<String>,
}

pub(crate) struct Marks<'a> {
    author: &'a str,
    date: Option<&'a str>,
    high: u32,
}

impl Marks<'_> {
    pub(crate) fn write<'a, E: ElementSpec>(
        &mut self,
        mut mark: LensMut<'a, E>,
    ) -> Result<LensMut<'a, E>> {
        self.high = self
            .high
            .checked_add(1)
            .filter(|&id| id <= i32::MAX.cast_unsigned())
            .ok_or(Error::AnnotationIdsExhausted)?;
        mark.set_attr(
            w("id"),
            self.high.format_into(&mut core::fmt::NumBuffer::new()),
        )?;
        mark.set_attr(w("author"), self.author)?;
        if let Some(date) = self.date {
            mark.set_attr(w("date"), date)?;
        }
        Ok(mark)
    }
}

// The snapshot types have narrower schemas than the live properties.
// The generator only derives SameType for identical SDK complex types.
// These projections keep the snapshot element's schema and use a checked
// detach after rejecting revisions and, for pPr, omitting live-only children.
macro_rules! property_snapshot {
    ($snapshot:ident, $schema:ty, $live:ty, $parent:ty) => {
        enum $snapshot {}
        impl ElementSpec for $snapshot {
            const QNAME: QName = <$schema>::QNAME;
            const SDK_NAME: &'static str = <$schema>::SDK_NAME;
            const ATTRS: &'static [ooxml_core::AttrSpec] = <$schema>::ATTRS;
            const CONTENT: ooxml_core::Content = <$schema>::CONTENT;
        }
        impl ooxml_core::schema::__private::Sealed for $snapshot {}
        impl ooxml_core::Detachable for $snapshot {}
        impl ooxml_core::SameType<$live> for $snapshot {}
        impl ChildOf<$parent> for $snapshot {}
    };
}

property_snapshot!(
    RunPropertySnapshot,
    PreviousRunProperties,
    RunProperties,
    RunPropertiesChange
);
property_snapshot!(
    ParagraphPropertySnapshot,
    ParagraphPropertiesExtended,
    ParagraphProperties,
    ParagraphPropertiesChange
);

impl Document {
    /// Starts a tracked editing session, also when the document has W1 enabled.
    /// The optional date is written as local wall-clock time with a Z suffix.
    pub fn track_changes(
        &mut self,
        author: Author,
        date: Option<LocalDateTime>,
    ) -> Result<TrackedChanges<'_>> {
        self.tracked_protection_gate()?;
        if author.0.is_empty() {
            return Err(Error::InvalidValue {
                what: "revision author",
                value: author.0,
            });
        }
        map_text(&author.0)?;
        let date = date
            .map(|date| dates(Some(date)).map(|(local, _)| local))
            .transpose()?;
        Ok(TrackedChanges {
            doc: self,
            author,
            date,
        })
    }
}

impl TrackedChanges<'_> {
    pub(crate) fn edit<T>(
        &mut self,
        story: StoryIndex,
        f: impl FnOnce(&mut XmlDocument, &mut Marks<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut high = self.doc.annotation_high_water;
        for part in self.doc.registry_parts(&[])? {
            if let Some(max) = descendants(self.doc.pkg.xml_part(&part)?.root())
                .filter_map(annotation_id)
                .max()
            {
                high = high.max(max);
            }
        }
        let part = &self.doc.stories[story.get()].id.part;
        let mut xml = self
            .doc
            .pkg
            .parsed_part(part)
            .expect("story parsed at open")
            .clone();
        let mut marks = Marks {
            high,
            author: &self.author.0,
            date: self.date.as_deref(),
        };
        let result = f(&mut xml, &mut marks)?;
        let mut plan = RegistryPlan::new(&self.doc.pkg);
        plan.stage_part(&self.doc.pkg, part, xml)?;
        self.doc.pkg.apply(plan)?;
        self.doc.annotation_high_water = marks.high;
        Ok(result)
    }

    fn run_path(&self, at: RunRef) -> Result<Vec<usize>> {
        let paragraph = self
            .doc
            .paragraph_at(at.para)
            .ok_or(Error::NoSuchParagraph(at.para.index))?;
        let run = paragraph.run(at.run).ok_or(Error::NoSuchRun {
            paragraph: at.para.index,
            run: at.run,
        })?;
        Ok(paragraph.data.index.path(run.info.frame))
    }

    fn para_path(&self, at: ParaRef) -> Result<Vec<usize>> {
        let p = self
            .doc
            .paragraph_at(at)
            .ok_or(Error::NoSuchParagraph(at.index))?;
        if p.info.in_revision_range {
            return Err(Error::InsideRevision {
                context: RunContext::Direct,
            });
        }
        Ok(p.data.index.path(p.info.frame))
    }

    /// Replaces a main-story run with a deletion and an insertion.
    pub fn set_run_text(&mut self, paragraph: usize, run: usize, text: &str) -> Result<()> {
        self.set_run_text_at(
            RunRef::new(ParaRef::new(StoryIndex::MAIN, paragraph), run),
            text,
        )
    }

    /// Replaces a story run with a deletion and an insertion.
    pub fn set_run_text_at(&mut self, at: RunRef, text: &str) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Text)?;
        let path = self.run_path(at)?;
        let run = self.doc.run_at(at).expect("resolved run path");
        run.check_settable()?;
        let pieces = map_text(text)?;
        if current_pieces(run.raw()).as_ref() == Some(&pieces) && !lacks_needed_preserve(run.raw())
        {
            return Ok(());
        }
        self.edit(at.para.story, |xml, marks| {
            let run = xml.lens_at::<R>(&path)?;
            let old = run.move_source();
            let rpr = if pieces.is_empty() {
                None
            } else {
                run.child::<RunProperties>()
                    .map(|lens| lens.detach_as::<RunProperties>(&[]))
                    .transpose()?
            };
            let (&item, parent) = path.split_last().expect("run has parent");
            if node_at(xml.root(), parent).is(Hyperlink::QNAME) {
                replace_run(
                    xml.lens_mut_at::<Hyperlink>(parent)?,
                    item,
                    &old,
                    rpr.as_ref(),
                    &pieces,
                    marks,
                )?;
            } else {
                replace_run(
                    xml.lens_mut_at::<P>(parent)?,
                    item,
                    &old,
                    rpr.as_ref(),
                    &pieces,
                    marks,
                )?;
            }
            Ok(())
        })?;
        self.doc.reindex(at.para.story);
        self.doc.bump(at.para);
        Ok(())
    }

    /// Inserts plain text after a run, or at the end of a paragraph.
    pub fn insert_text(&mut self, at: RunPosition, text: &str) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Text)?;
        let para = at.para();
        let pieces = map_text(text)?;
        if pieces.is_empty() {
            return Ok(());
        }
        let path = self.para_path(para)?;
        check_path(self.doc.story_root_node(para.story), &path)?;
        self.doc.para_location(para)?;
        self.doc.check_structure_neighbour(para)?;
        let root = self.doc.story_root_node(para.story);
        let (parent, index) = match at {
            RunPosition::EndOf(_) => {
                let end = node_at(root, &path).children().len();
                (path, end)
            }
            RunPosition::After(at) => {
                let mut path = self.run_path(at)?;
                self.doc
                    .run_at(at)
                    .expect("resolved run path")
                    .check_settable()?;
                let item = path.pop().expect("run has a parent");
                (path, item + 1)
            }
        };
        self.edit(para.story, |xml, marks| {
            if node_at(xml.root(), &parent).is(Hyperlink::QNAME) {
                insert_run(
                    xml.lens_mut_at::<Hyperlink>(&parent)?,
                    index,
                    None,
                    &pieces,
                    marks,
                )
            } else {
                insert_run(xml.lens_mut_at::<P>(&parent)?, index, None, &pieces, marks)
            }
        })?;
        self.doc.reindex(para.story);
        self.doc.bump(para);
        Ok(())
    }

    /// Marks a range of text as deleted without deleting paragraph marks.
    /// Byte offsets, when supplied, must be UTF-8 boundaries within their runs.
    pub fn delete_text(&mut self, range: TextRange) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Text)?;
        let first = range.first;
        let last = range.last;
        if first.para.story != last.para.story
            || (first.para.index, first.run) > (last.para.index, last.run)
        {
            return Err(Error::InvalidTextRange);
        }
        let story = first.para.story;
        let mut deletions = Vec::new();
        self.run_path(first)?;
        self.run_path(last)?;
        for i in first.para.index..=last.para.index {
            let para = self
                .doc
                .paragraph_at(ParaRef::new(story, i))
                .ok_or(Error::InvalidTextRange)?;
            let from = if i == first.para.index { first.run } else { 0 };
            let to = if i == last.para.index {
                last.run + 1
            } else {
                para.run_count()
            };
            for run in para.runs().skip(from).take(to - from) {
                run.check_settable()?;
                let text = run.text();
                let at = run.run_ref();
                let start = if at == first {
                    range.start_byte.unwrap_or(0)
                } else {
                    0
                };
                let end = if at == last {
                    range.end_byte.unwrap_or(text.len())
                } else {
                    text.len()
                };
                if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                    return Err(Error::InvalidTextRange);
                }
                if start < end {
                    deletions.push((self.run_path(at)?, start, end, text.len()));
                }
            }
        }
        if deletions.is_empty() {
            return Err(Error::InvalidTextRange);
        }
        self.edit(story, |xml, marks| {
            for (path, start, end, len) in deletions.iter().rev() {
                let (&item, parent) = path.split_last().expect("run has parent");
                if node_at(xml.root(), parent).is(Hyperlink::QNAME) {
                    delete_run_slice::<Hyperlink>(xml, parent, item, *start..*end, *len, marks)?;
                } else {
                    delete_run_slice::<P>(xml, parent, item, *start..*end, *len, marks)?;
                }
            }
            Ok(())
        })?;
        self.doc.reindex(story);
        for i in first.para.index..=last.para.index {
            self.doc.bump(ParaRef::new(story, i));
        }
        Ok(())
    }

    /// Records the previous run properties and applies direct formatting.
    pub fn set_run_properties(&mut self, at: RunRef, format: &RunFormat) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Format)?;
        let path = self.run_path(at)?;
        check_path(self.doc.story_root_node(at.para.story), &path)?;
        self.doc.check_formattable_run(at)?;
        let (old, prefix_path) = self.doc.run_container(at)?;
        let plan = plan_run(old, format)?;
        if plan.props.is_empty() {
            return Ok(());
        }
        self.doc
            .check_prefixes(at.para.story, &prefix_path, old, &plan)?;
        self.edit(at.para.story, |xml, marks| {
            let old = xml
                .lens_at::<R>(&path)?
                .child::<RunProperties>()
                .map(|old| old.detach_as::<RunPropertySnapshot>(&[]))
                .transpose()?;
            let mut run = xml.lens_mut_at::<R>(&path)?;
            let mut rpr = run.get_or_insert::<RunProperties>()?;
            for op in &plan.props {
                apply_rpr(&mut rpr, op)?;
            }
            let mut change =
                marks.write(rpr.insert_with_ns::<RunPropertiesChange>(REVISION_NS)?)?;
            if let Some(old) = old {
                change.insert_detached(&old)?;
            } else {
                change.insert::<PreviousRunProperties>()?;
            }
            Ok(())
        })?;
        self.doc.reindex(at.para.story);
        self.doc.bump(at.para);
        Ok(())
    }

    /// Records the previous paragraph properties and applies direct formatting.
    pub fn set_paragraph_properties(
        &mut self,
        at: ParaRef,
        format: &ParagraphFormat,
    ) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Format)?;
        let path = self.para_path(at)?;
        check_path(self.doc.story_root_node(at.story), &path)?;
        self.doc.check_formattable_paragraph(at)?;
        let (old, prefix_path) = self.doc.para_container(at)?;
        let style = old
            .and_then(|p| p.elements().find(|n| n.is(w("pStyle"))))
            .and_then(|s| s.attr(VAL));
        let inherited = |el, attr| self.doc.inherited_ppr_attr(style, el, attr);
        let plan = plan_paragraph(old, format, &inherited)?;
        if plan.props.is_empty() {
            return Ok(());
        }
        self.doc
            .check_prefixes(at.story, &prefix_path, old, &plan)?;
        self.edit(at.story, |xml, marks| {
            let old = xml
                .lens_at::<P>(&path)?
                .child::<ParagraphProperties>()
                .map(|old| {
                    old.detach_as::<ParagraphPropertySnapshot>(&[
                        SectionProperties::QNAME,
                        w("rPr"),
                        w("pPrChange"),
                    ])
                })
                .transpose()?;
            let mut p = xml.lens_mut_at::<P>(&path)?;
            let mut ppr = p.get_or_insert::<ParagraphProperties>()?;
            for op in &plan.props {
                apply_ppr(&mut ppr, op)?;
            }
            let mut change =
                marks.write(ppr.insert_with_ns::<ParagraphPropertiesChange>(REVISION_NS)?)?;
            if let Some(old) = old {
                change.insert_detached(&old)?;
            } else {
                change.insert::<ParagraphPropertiesExtended>()?;
            }
            Ok(())
        })?;
        self.doc.reindex(at.story);
        self.doc.bump(at);
        Ok(())
    }
}

fn delete_run_slice<E: ElementSpec>(
    xml: &mut XmlDocument,
    parent: &[usize],
    mut item: usize,
    range: std::ops::Range<usize>,
    len: usize,
    marks: &mut Marks<'_>,
) -> Result<()>
where
    R: ChildOf<E>,
    InsertedRun: ChildOf<E>,
    DeletedRun: ChildOf<E>,
{
    if range.end < len {
        split_boundary::<E>(
            xml,
            parent,
            Boundary::Inside {
                child: item,
                byte: range.end,
            },
        )?;
    }
    if range.start > 0 {
        item = split_boundary::<E>(
            xml,
            parent,
            Boundary::Inside {
                child: item,
                byte: range.start,
            },
        )?;
    }
    let old = xml.lens_at::<R>(&[parent, &[item]].concat())?.move_source();
    replace_run(xml.lens_mut_at::<E>(parent)?, item, &old, None, &[], marks)
}

fn replace_run<E: ElementSpec>(
    mut parent: LensMut<'_, E>,
    item: usize,
    old: &ooxml_core::lens::MoveSource<R>,
    rpr: Option<&ooxml_core::lens::Detached<RunProperties>>,
    pieces: &[Piece],
    marks: &mut Marks<'_>,
) -> Result<()>
where
    DeletedRun: ChildOf<E>,
    InsertedRun: ChildOf<E>,
{
    let mut deletion =
        marks.write(parent.insert_at_with_ns::<DeletedRun>(item + 1, REVISION_NS)?)?;
    rename_deleted_text(&mut deletion.move_in::<R>(old)?)?;
    if !pieces.is_empty() {
        insert_run(parent, item + 1, rpr, pieces, marks)?;
    }
    Ok(())
}

fn insert_run<E: ElementSpec>(
    mut parent: LensMut<'_, E>,
    index: usize,
    rpr: Option<&ooxml_core::lens::Detached<RunProperties>>,
    pieces: &[Piece],
    marks: &mut Marks<'_>,
) -> Result<()>
where
    InsertedRun: ChildOf<E>,
{
    let mut insertion =
        marks.write(parent.insert_at_with_ns::<InsertedRun>(index, REVISION_NS)?)?;
    let mut run = insertion.insert::<R>()?;
    if let Some(rpr) = rpr {
        run.insert_detached(rpr)?;
    }
    write_pieces(&mut run, pieces)
}

fn rename_deleted_text(run: &mut LensMut<'_, R>) -> Result<()> {
    let text: Vec<_> = run
        .node()
        .indexed_elements()
        .filter(|(_, node)| node.is(Text::QNAME) || node.is(FieldCode::QNAME))
        .map(|(i, node)| {
            (
                i,
                node.is(FieldCode::QNAME),
                node.text().into_owned(),
                node.attr(XML_SPACE).map(str::to_owned),
            )
        })
        .collect();
    for (i, instruction, text, space) in text {
        run.remove_at(i);
        if instruction {
            write_deleted(
                run.insert_at::<DeletedFieldCode>(i)?,
                &text,
                space.as_deref(),
            )?;
        } else {
            write_deleted(run.insert_at::<DeletedText>(i)?, &text, space.as_deref())?;
        }
    }
    Ok(())
}

fn write_deleted<E: ElementSpec>(
    mut node: LensMut<'_, E>,
    text: &str,
    space: Option<&str>,
) -> Result<()> {
    node.set_text(text)?;
    if let Some(space) = space {
        node.set_attr(XML_SPACE, space)?;
    }
    Ok(())
}

impl TrackedChanges<'_> {
    /// Appends a paragraph and marks the previous paragraph mark as inserted.
    pub fn append_paragraph(&mut self, text: &str) -> Result<usize> {
        self.doc.tracked_write_gate(Operation::Append)?;
        let story = StoryIndex::MAIN;
        let index = &self.doc.stories[0].index;
        let body = index.body.ok_or(Error::NoBody)?;
        if index.open_fields_at_end > 0 {
            return Err(Error::InsideField);
        }
        let root = self.doc.story_root_node(story);
        let parent = node_at(root, &[body]);
        let pos = parent
            .indexed_elements()
            .find(|(_, n)| n.is(SectionProperties::QNAME))
            .map_or(parent.children().len(), |(i, _)| i);
        let previous = parent
            .indexed_elements()
            .take_while(|(i, _)| *i < pos)
            .last()
            .map(|(i, _)| i);
        let previous = previous
            .filter(|&i| {
                parent.children()[i]
                    .as_element()
                    .is_some_and(|n| n.is(P::QNAME))
            })
            .ok_or(Error::NoFollowingParagraph)?;
        let at = ParaRef::new(story, index.paragraphs.len());
        self.insert_paragraph_at(at, &[body], pos, Some(previous), text)?;
        Ok(at.index)
    }

    /// Inserts a paragraph with the previous paragraph's mark properties.
    /// A second insertion at the start of the container touches the first
    /// insertion's revision and is refused.
    pub fn insert_paragraph(
        &mut self,
        place: Place,
        text: &str,
        spec: &ParagraphSpec,
    ) -> Result<ParaRef> {
        self.doc.tracked_write_gate(Operation::Structure)?;
        if !matches!(spec, ParagraphSpec::Plain) {
            return Err(Error::TrackedParagraphFormatting);
        }
        let (anchor, before) = match place {
            Place::Before(at) => (at, true),
            Place::After(at) => (at, false),
        };
        let path = self.para_path(anchor)?;
        check_path(self.doc.story_root_node(anchor.story), &path)?;
        self.doc.check_structure_neighbour(anchor)?;
        let (parent_path, item) = self.doc.para_location(anchor)?;
        let parent = node_at(self.doc.story_root_node(anchor.story), &parent_path);
        let pos = if before {
            item
        } else {
            next_slot_sibling(parent, item)
        };
        let previous = parent
            .indexed_elements()
            .take_while(|(i, _)| *i < pos)
            .last()
            .filter(|(_, n)| n.is(P::QNAME))
            .map(|(i, _)| i);
        let at = ParaRef::new(anchor.story, anchor.index + usize::from(!before));
        self.insert_paragraph_at(at, &parent_path, pos, previous, text)?;
        Ok(at)
    }

    fn insert_paragraph_at(
        &mut self,
        at: ParaRef,
        parent: &[usize],
        pos: usize,
        previous: Option<usize>,
        text: &str,
    ) -> Result<()> {
        let story = at.story;
        let pieces = map_text(text)?;
        let root = self.doc.story_root_node(story);
        let container = node_at(root, parent);
        let previous_path = previous.map(|previous| [parent, &[previous]].concat());
        if let Some(path) = &previous_path {
            check_path(root, path)?;
            let prev = ParaRef::new(story, at.index - 1);
            self.doc.check_structure_neighbour(prev)?;
        } else if !container
            .children()
            .get(pos)
            .and_then(|item| item.as_element())
            .is_some_and(|n| n.is(P::QNAME))
        {
            return Err(Error::NoFollowingParagraph);
        }
        let story_kind = self.doc.stories[story.get()].id.kind;
        self.edit(story, |xml, marks| {
            let old = previous_path
                .as_ref()
                .map(|path| xml.lens_at::<P>(path))
                .transpose()?
                .and_then(|p| p.child::<ParagraphProperties>());
            let copy = old
                .as_ref()
                .map(|ppr| ppr.detach_as::<ParagraphProperties>(&[SectionProperties::QNAME]))
                .transpose()?;
            let original = old.map(|ppr| ppr.move_source());
            let mut container =
                container_at(xml, story_kind, parent)?.ok_or(Error::NoSuchParagraph(at.index))?;
            on_container!(&mut container, lens => {
                let mut inserted = lens.insert_at::<P>(pos)?;
                if let Some(original) = original {
                    inserted.move_in::<ParagraphProperties>(&original)?;
                }
                if previous_path.is_none() {
                    mark_paragraph::<Inserted>(&mut inserted, marks)?;
                }
                if !pieces.is_empty() {
                    let mut ins = marks.write(inserted.insert_with_ns::<InsertedRun>(REVISION_NS)?)?;
                    write_pieces(&mut ins.insert::<R>()?, &pieces)?;
                }
            });
            if let Some(path) = &previous_path {
                let mut p = xml.lens_mut_at::<P>(path)?;
                if let Some(copy) = copy {
                    p.insert_detached(&copy)?;
                }
                mark_paragraph::<Inserted>(&mut p, marks)?;
            }
            Ok(())
        })?;
        self.doc.paragraph_inserted(at);
        if previous.is_some() {
            self.doc.bump(ParaRef::new(story, at.index - 1));
        }
        Ok(())
    }

    /// Deletes a paragraph's text and mark, joining it to its following paragraph on acceptance.
    pub fn delete_paragraph(&mut self, at: ParaRef) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Structure)?;
        let path = self.para_path(at)?;
        let root = self.doc.story_root_node(at.story);
        check_path(root, &path)?;
        let (parent, item) = self.doc.para_location(at)?;
        self.doc.check_structure_neighbour(at)?;
        let container = node_at(root, &parent);
        let next = next_slot_sibling(container, item);
        if !container
            .children()
            .get(next)
            .and_then(|n| n.as_element())
            .is_some_and(|n| n.is(P::QNAME))
        {
            return Err(Error::NoFollowingParagraph);
        }
        check_path(root, &[parent.as_slice(), &[next]].concat())?;
        self.doc
            .check_structure_neighbour(ParaRef::new(at.story, at.index + 1))?;
        let p = self.doc.paragraph_at(at).expect("resolved paragraph");
        check_nodes(descendants(p.raw()))?;
        for run in p.runs() {
            run.check_settable()?;
        }
        check_deletable_paragraph(p.raw())?;
        self.edit(at.story, |xml, marks| {
            delete_paragraph_runs(xml, &path, marks)?;
            mark_paragraph::<Deleted>(&mut xml.lens_mut_at::<P>(&path)?, marks)
        })?;
        self.doc.reindex(at.story);
        self.doc.bump(at);
        Ok(())
    }
}

pub(crate) fn mark_paragraph<E: ChildOf<ParagraphMarkRunProperties>>(
    p: &mut LensMut<'_, P>,
    marks: &mut Marks<'_>,
) -> Result<()> {
    let mut ppr = p.get_or_insert::<ParagraphProperties>()?;
    let mut rpr = ppr.get_or_insert::<ParagraphMarkRunProperties>()?;
    marks.write(rpr.insert_at_with_ns::<E>(0, REVISION_NS)?)?;
    Ok(())
}

pub(crate) fn check_deletable_paragraph(p: &RawNode) -> Result<()> {
    for child in p.elements() {
        if child.is(ParagraphProperties::QNAME) || child.is(R::QNAME) {
            continue;
        }
        if child.is(Hyperlink::QNAME) && child.elements().all(|run| run.is(R::QNAME)) {
            continue;
        }
        return Err(Error::RunNotTextOnly {
            child: child.name().local().to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn delete_paragraph_runs(
    xml: &mut XmlDocument,
    path: &[usize],
    marks: &mut Marks<'_>,
) -> Result<()> {
    let mut paths = Vec::new();
    for (i, child) in node_at(xml.root(), path).indexed_elements() {
        if child.is(R::QNAME) {
            paths.push([path, &[i]].concat());
        } else if child.is(Hyperlink::QNAME) {
            paths.extend(
                child
                    .indexed_elements()
                    .filter(|(_, n)| n.is(R::QNAME))
                    .map(|(j, _)| [path, &[i, j]].concat()),
            );
        }
    }
    for path in paths.iter().rev() {
        let old = xml.lens_at::<R>(path)?.move_source();
        let (&item, parent) = path.split_last().expect("run parent");
        if node_at(xml.root(), parent).is(Hyperlink::QNAME) {
            replace_run(
                xml.lens_mut_at::<Hyperlink>(parent)?,
                item,
                &old,
                None,
                &[],
                marks,
            )?;
        } else {
            replace_run(xml.lens_mut_at::<P>(parent)?, item, &old, None, &[], marks)?;
        }
    }
    Ok(())
}
