//! Atomic Word comment edits (ADR-0015 §3).
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::NumBuffer;

use chrono::{Datelike, Local, NaiveDateTime, TimeDelta};
use ooxml_core::{
    ChildOf, ElementSpec, NsDecl, QName, RawItem, RawNode, RegistryPlan, XmlDocument, name::MC_NS,
};
use ooxml_schema::wml::{self as schema, *};

use crate::bookmarks::{Boundary, annotation_id, split_boundary};
use crate::document::{node_at, write_pieces};
use crate::names::{W_ID, descendants, w};
use crate::notes::{named_annotation_style, styled_run};
use crate::text::{map_text, run_text};
use crate::{Document, Error, ParaRef, Result, RunContext, RunRef, StoryKind};

const LEGACY: &str = "http://schemas.microsoft.com/office/word/2010/11/wordml";
const COMMENTS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
pub(crate) const EX_REL: &str =
    "http://schemas.microsoft.com/office/2011/relationships/commentsExtended";
const IDS_REL: &str = "http://schemas.microsoft.com/office/2016/09/relationships/commentsIds";
const CEX_REL: &str =
    "http://schemas.microsoft.com/office/2018/08/relationships/commentsExtensible";
const PEOPLE_REL: &str = "http://schemas.microsoft.com/office/2011/relationships/people";
const IGNORABLE: QName = QName::new(MC_NS, "Ignorable");
const PARA: QName = QName::new(NS_W14, "paraId");
const TEXT: QName = QName::new(NS_W14, "textId");
const EX_PARA: QName = QName::new(NS_W15, "paraId");
const EX_PARENT: QName = QName::new(NS_W15, "paraIdParent");
const DONE: QName = QName::new(NS_W15, "done");
const CID_PARA: QName = QName::new(NS_W16CID, "paraId");
const CID_DURABLE: QName = QName::new(NS_W16CID, "durableId");
const CEX_DURABLE: QName = QName::new(NS_W16CEX, "durableId");
const COMMENT_PARAGRAPH_NS: &[NsDecl<'_>] = &[
    NsDecl {
        prefix: "w14",
        uri: NS_W14,
    },
    NsDecl {
        prefix: "mc",
        uri: MC_NS,
    },
];
const W: &[NsDecl<'_>] = &[NsDecl {
    prefix: "w",
    uri: NS_W,
}];

/// A Word comment's annotation id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommentId(pub i32);

/// Local wall-clock time and its signed offset east of UTC, in minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalDateTime {
    pub wall_clock: NaiveDateTime,
    pub utc_offset_minutes: i32,
}

/// Author, optional metadata, and plain-text paragraphs for a new comment.
#[derive(Clone, Debug)]
pub struct NewComment {
    pub author: String,
    pub initials: Option<String>,
    pub date: Option<LocalDateTime>,
    pub paragraphs: Vec<String>,
}

/// A nonempty text range within one story, possibly crossing paragraphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub(crate) first: RunRef,
    pub(crate) last: RunRef,
    pub(crate) start_byte: Option<usize>,
    pub(crate) end_byte: Option<usize>,
}

impl TextRange {
    /// Covers the first through the last run, inclusive.
    pub fn new(first: RunRef, last: RunRef) -> Self {
        Self {
            first,
            last,
            start_byte: None,
            end_byte: None,
        }
    }

    /// Covers text from the UTF-8 byte offset in `first` to the exclusive byte
    /// offset in `last`. A boundary inside a run splits that run.
    pub fn with_offsets(first: RunRef, start_byte: usize, last: RunRef, end_byte: usize) -> Self {
        Self {
            first,
            last,
            start_byte: Some(start_byte),
            end_byte: Some(end_byte),
        }
    }
}

/// The anchor elements in a related story part. Point comments have only a reference.
#[derive(Clone, Debug)]
pub struct CommentAnchor<'a> {
    pub part: &'a str,
    pub start: Option<&'a RawNode>,
    pub end: Option<&'a RawNode>,
    pub reference: Option<&'a RawNode>,
}

/// A comment definition and the metadata associated with its last paragraph.
#[derive(Clone, Debug)]
pub struct CommentRef<'a> {
    pub id: CommentId,
    pub author: &'a str,
    pub initials: Option<&'a str>,
    pub date: Option<&'a str>,
    pub text: String,
    pub parent: Option<CommentId>,
    pub done: bool,
    pub anchor: Option<CommentAnchor<'a>>,
}

#[derive(Default)]
struct Parts {
    comments: Option<String>,
    extended: Option<String>,
    ids: Option<String>,
    extensible: Option<String>,
    people: Option<String>,
}

#[derive(Clone, Copy)]
struct Definition<'a> {
    id: CommentId,
    index: usize,
    node: &'a RawNode,
    para: Option<u32>,
    parent_para: Option<u32>,
    done: bool,
}

fn hex(value: Option<&str>) -> Option<u32> {
    u32::from_str_radix(value?, 16).ok()
}

fn id(node: &RawNode) -> Option<CommentId> {
    node.attr(W_ID)?.trim().parse().ok().map(CommentId)
}

fn definitions<'a>(comments: &'a RawNode, extended: Option<&RawNode>) -> Vec<Definition<'a>> {
    let mut entries = BTreeMap::new();
    if let Some(root) = extended {
        let ns = if root.is(CommentsEx::QNAME) {
            NS_W15
        } else {
            LEGACY
        };
        for entry in root
            .elements()
            .filter(|n| n.is(QName::new(ns, "commentEx")))
        {
            if let Some(para) = hex(entry.attr(QName::new(ns, "paraId"))) {
                let parent = hex(entry.attr(QName::new(ns, "paraIdParent")));
                let done = entry
                    .attr(QName::new(ns, "done"))
                    .and_then(ooxml_core::value::parse_on_off);
                entries
                    .entry(para)
                    .or_insert((parent, done.unwrap_or(false)));
            }
        }
    }
    comments
        .indexed_elements()
        .filter_map(|(index, node)| {
            if !node.is(Comment::QNAME) {
                return None;
            }
            let para = comment_paragraphs(node)
                .last()
                .and_then(|(_, node)| hex(node.attr(PARA)));
            let (parent_para, done) = para
                .and_then(|p| entries.get(&p).copied())
                .unwrap_or_default();
            Some(Definition {
                id: id(node)?,
                index,
                node,
                para,
                parent_para,
                done,
            })
        })
        .collect()
}

fn definition<'a>(defs: &[Definition<'a>], id: CommentId) -> Result<Definition<'a>> {
    let mut found = defs.iter().filter(|def| def.id == id);
    let def = *found.next().ok_or(Error::NoSuchComment(id.0))?;
    if found.next().is_some()
        || def.para.is_some_and(|para| {
            defs.iter()
                .any(|other| other.id != id && other.para == Some(para))
        })
    {
        return Err(Error::InvalidCommentPart);
    }
    Ok(def)
}

fn thread_root<'a>(defs: &[Definition<'a>], id: CommentId) -> Result<Definition<'a>> {
    let mut current = definition(defs, id)?;
    let mut visited = BTreeSet::new();
    while let Some(parent_para) = current.parent_para {
        if !visited.insert(current.id) {
            return Err(Error::InvalidCommentPart);
        }
        let parent = defs
            .iter()
            .find(|def| def.para == Some(parent_para))
            .ok_or(Error::InvalidCommentPart)?;
        current = definition(defs, parent.id)?;
    }
    Ok(current)
}

fn thread(defs: &[Definition<'_>], root: Definition<'_>) -> BTreeSet<CommentId> {
    let mut ids = BTreeSet::from([root.id]);
    let mut paras: BTreeSet<_> = root.para.into_iter().collect();
    loop {
        let previous = ids.len();
        for def in defs {
            if def.parent_para.is_some_and(|p| paras.contains(&p)) && ids.insert(def.id) {
                paras.extend(def.para);
            }
        }
        if previous == ids.len() {
            return ids;
        }
    }
}

fn walk(root: &RawNode) -> impl Iterator<Item = (Vec<usize>, &RawNode)> {
    let mut pending = vec![(Vec::new(), root)];
    std::iter::from_fn(move || {
        let (path, node) = pending.pop()?;
        let old_len = pending.len();
        pending.extend(node.indexed_elements().map(|(i, child)| {
            let mut next = path.clone();
            next.push(i);
            (next, child)
        }));
        pending[old_len..].reverse();
        Some((path, node))
    })
}

fn comment_paragraphs(comment: &RawNode) -> impl Iterator<Item = (Vec<usize>, &RawNode)> {
    walk(comment).filter(|(_, node)| node.is(Paragraph::QNAME))
}

fn anchor_kind(node: &RawNode) -> Option<usize> {
    [
        CommentRangeStart::QNAME,
        CommentRangeEnd::QNAME,
        CommentReference::QNAME,
    ]
    .iter()
    .position(|q| node.is(*q))
}

/// Verify an ignorable URI against the prefix the lens will actually use.
fn require_ignorable(root: &RawNode, path: &[usize], uri: &'static str) -> Result<()> {
    let prefixes = root.attr(IGNORABLE).unwrap_or_default();
    let mut declarations = Vec::new();
    let mut current = root;
    for (depth, next) in path
        .iter()
        .copied()
        .map(Some)
        .chain(std::iter::once(None))
        .enumerate()
    {
        declarations.extend(
            current
                .attrs()
                .iter()
                .filter(|attr| attr.name().is_namespace_declaration())
                .map(|attr| (depth, attr.name().declared_prefix(), attr.value())),
        );
        if let Some(i) = next {
            current = current.children()[i].as_element().expect("element path");
        }
    }
    let effective = |prefix| {
        declarations
            .iter()
            .rev()
            .find(|(_, p, _)| *p == prefix)
            .map(|(_, _, uri)| *uri)
    };
    let eligible = |prefix: Option<&str>| {
        prefix.is_some_and(|prefix| {
            prefixes.split_ascii_whitespace().any(|p| p == prefix)
                && declarations
                    .iter()
                    .any(|&(depth, p, value)| depth == 0 && p == Some(prefix) && value == uri)
        })
    };
    let attribute = declarations
        .iter()
        .rev()
        .find(|&&(_, prefix, value)| {
            prefix.is_some() && value == uri && effective(prefix) == Some(uri)
        })
        .and_then(|&(_, prefix, _)| prefix);
    // For extension elements we insert below the part root. The lens first
    // tries the root's prefix, then the default binding, then the first alias.
    let element_ok = uri == NS_W14 || {
        let root_prefix = root.name().prefix();
        let element = if effective(root_prefix) == Some(uri) {
            root_prefix
        } else if effective(None) == Some(uri) {
            None
        } else {
            declarations
                .iter()
                .find(|&&(_, prefix, value)| value == uri && effective(prefix) == Some(uri))
                .and_then(|&(_, prefix, _)| prefix)
        };
        eligible(element)
    };
    if !eligible(attribute) || !element_ok {
        return Err(Error::NotIgnorable(uri));
    }
    Ok(())
}

fn random_hex(used: &mut BTreeSet<u32>) -> Result<String> {
    loop {
        let mut bytes = [0; 4];
        getrandom::fill(&mut bytes).map_err(|_| Error::CommentIdRandomness)?;
        let value = u32::from_ne_bytes(bytes) & 0x7fff_ffff;
        if value != 0 && used.insert(value) {
            return Ok(format!("{value:08X}"));
        }
    }
}

pub(crate) fn dates(date: Option<LocalDateTime>) -> Result<(String, String)> {
    let (local, utc) = if let Some(date) = date {
        let utc = date
            .wall_clock
            .checked_sub_signed(TimeDelta::minutes(i64::from(date.utc_offset_minutes)))
            .filter(|_| (-1439..=1439).contains(&date.utc_offset_minutes))
            .ok_or_else(|| Error::InvalidValue {
                what: "UTC offset",
                value: format!("{}", date.utc_offset_minutes),
            })?;
        (date.wall_clock, utc)
    } else {
        let now = Local::now();
        (now.naive_local(), now.naive_utc())
    };
    for date in [local, utc] {
        if !(0..=9999).contains(&date.year()) {
            return Err(Error::InvalidValue {
                what: "date year",
                value: date.year().to_string(),
            });
        }
    }
    let format = |date: NaiveDateTime| date.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    Ok((format(local), format(utc)))
}

impl Document {
    fn comment_part(&self, relationship: &str) -> Result<Option<String>> {
        let rels = self.pkg.relationships(self.main_part_name())?;
        let mut found = rels.by_type(relationship);
        let Some(first) = found.next() else {
            return Ok(None);
        };
        if found.next().is_some() {
            return Err(Error::InvalidCommentPart);
        }
        rels.resolve(first)
            .filter(|part| self.pkg.contains(part))
            .map(Some)
            .ok_or(Error::InvalidCommentPart)
    }

    fn comment_parts(&self) -> Result<Parts> {
        Ok(Parts {
            comments: self.comment_part(COMMENTS_REL)?,
            extended: self.comment_part(EX_REL)?,
            ids: self.comment_part(IDS_REL)?,
            extensible: self.comment_part(CEX_REL)?,
            people: self.comment_part(PEOPLE_REL)?,
        })
    }

    /// Reads comment definitions in part order, including legacy threads and point anchors.
    ///
    /// Returns an empty iterator if the comments part is missing, unreadable,
    /// or cannot be identified unambiguously. Unreadable extended metadata is ignored.
    pub fn comments(&self) -> impl Iterator<Item = CommentRef<'_>> {
        let comments = self
            .comment_part(COMMENTS_REL)
            .ok()
            .flatten()
            .and_then(|part| self.pkg.parsed_part(&part));
        let extended = self
            .comment_part(EX_REL)
            .ok()
            .flatten()
            .and_then(|part| self.pkg.parsed_part(&part))
            .map(XmlDocument::root);
        let defs = comments
            .filter(|xml| xml.root().is(Comments::QNAME))
            .map_or_default(|xml| definitions(xml.root(), extended));
        let mut anchors = BTreeMap::new();
        for story in self
            .stories()
            .filter(|story| story.id().kind() != StoryKind::Comments)
        {
            for (_, node) in walk(story.raw()) {
                let (Some(kind), Some(id)) = (anchor_kind(node), id(node)) else {
                    continue;
                };
                let item = anchors.entry(id).or_insert_with(|| CommentAnchor {
                    part: story.id().part(),
                    start: None,
                    end: None,
                    reference: None,
                });
                if item.part == story.id().part() {
                    match kind {
                        0 => item.start.get_or_insert(node),
                        1 => item.end.get_or_insert(node),
                        _ => item.reference.get_or_insert(node),
                    };
                }
            }
        }
        let parents: BTreeMap<_, _> = defs.iter().filter_map(|d| Some((d.para?, d.id))).collect();
        defs.into_iter().map(move |def| {
            let text = comment_paragraphs(def.node)
                .map(|(_, p)| {
                    walk(p)
                        .map(|(_, node)| node)
                        .filter(|n| n.is(Run::QNAME))
                        .map(run_text)
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            CommentRef {
                id: def.id,
                author: def.node.attr(w("author")).unwrap_or_default(),
                initials: def.node.attr(w("initials")),
                date: def.node.attr(w("date")),
                text,
                parent: def.parent_para.and_then(|p| parents.get(&p).copied()),
                done: def.done,
                anchor: anchors.remove(&def.id),
            }
        })
    }

    fn new_comment_xml(&self, extended: bool) -> Result<XmlDocument> {
        let root = if extended {
            format!(
                r#"<w15:commentsEx xmlns:w15="{NS_W15}" xmlns:mc="{MC_NS}" mc:Ignorable="w15"/>"#
            )
        } else {
            format!(
                r#"<w:comments xmlns:w="{NS_W}" xmlns:w14="{NS_W14}" xmlns:mc="{MC_NS}" mc:Ignorable="w14"/>"#
            )
        };
        Ok(XmlDocument::parse_with(
            root.as_bytes(),
            &self.pkg.parse_options(),
        )?)
    }

    fn load_comments(&mut self, parts: &Parts) -> Result<XmlDocument> {
        if let Some(name) = &parts.comments {
            let xml = self.pkg.xml_part(name)?;
            if !xml.root().is(Comments::QNAME) {
                return Err(Error::InvalidCommentPart);
            }
            return Ok(xml.clone());
        }
        self.new_comment_xml(false)
    }

    fn load_extended(&mut self, parts: &Parts, allow_legacy: bool) -> Result<Option<XmlDocument>> {
        if let Some(name) = &parts.extended {
            let xml = self.pkg.xml_part(name)?;
            if xml.root().is(CommentsEx::QNAME) {
                return Ok(Some(xml.clone()));
            }
            if xml.root().is(QName::new(LEGACY, "commentsEx")) {
                return if allow_legacy {
                    Ok(None)
                } else {
                    Err(Error::LegacyCommentsExtended)
                };
            }
            return Err(Error::InvalidCommentPart);
        }
        self.new_comment_xml(true).map(Some)
    }

    fn comment_ids(&mut self, parts: &Parts) -> Result<(u32, BTreeSet<u32>, BTreeSet<u32>)> {
        let mut high = self.annotation_high_water;
        let mut para = BTreeSet::new();
        let mut durable = BTreeSet::new();
        let mut scope = self.registry_parts(&[])?;
        scope.extend(
            parts
                .extended
                .iter()
                .chain(&parts.ids)
                .chain(&parts.extensible)
                .cloned(),
        );
        scope.sort_unstable();
        scope.dedup();
        for part in scope {
            for node in descendants(self.pkg.xml_part(&part)?.root()) {
                if let Some(id) = annotation_id(node) {
                    high = high.max(id);
                }
                if node.is(Paragraph::QNAME) || node.is(TableRow::QNAME) {
                    para.extend(hex(node.attr(PARA)));
                }
                for ns in [NS_W15, LEGACY, NS_W16CID] {
                    para.extend(hex(node.attr(QName::new(ns, "paraId"))));
                    para.extend(hex(node.attr(QName::new(ns, "paraIdParent"))));
                }
                durable.extend(hex(node.attr(CID_DURABLE)));
                durable.extend(hex(node.attr(CEX_DURABLE)));
            }
        }
        Ok((high, para, durable))
    }
}

type ChangedPaths = BTreeMap<String, Vec<Vec<usize>>>;

struct Location {
    part: String,
    start: Option<(Vec<usize>, Boundary)>,
    end: (Vec<usize>, Boundary),
    has_range: bool,
}

impl Location {
    fn changed_paths(self) -> ChangedPaths {
        let mut paths = vec![self.end.0];
        if let Some((start, _)) = self.start {
            paths.push(start);
        }
        BTreeMap::from([(self.part, paths)])
    }
}

fn put_anchor(xml: &mut XmlDocument, loc: &Location, id: CommentId, style: &str) -> Result<()> {
    let mut number = NumBuffer::new();
    let id = id.0.format_into(&mut number);
    let (path, boundary) = &loc.end;
    if node_at(xml.root(), path).is(Hyperlink::QNAME) {
        put_anchor_end::<Hyperlink>(xml, path, *boundary, loc.has_range, id, style)?;
    } else {
        put_anchor_end::<Paragraph>(xml, path, *boundary, loc.has_range, id, style)?;
    }
    if let Some((path, boundary)) = &loc.start {
        if node_at(xml.root(), path).is(Hyperlink::QNAME) {
            put_anchor_start::<Hyperlink>(xml, path, *boundary, id)?;
        } else {
            put_anchor_start::<Paragraph>(xml, path, *boundary, id)?;
        }
    }
    Ok(())
}

fn put_anchor_end<P: ElementSpec>(
    xml: &mut XmlDocument,
    path: &[usize],
    boundary: Boundary,
    has_range: bool,
    id: &str,
    style: &str,
) -> Result<()>
where
    Run: ChildOf<P>,
    CommentRangeEnd: ChildOf<P>,
{
    let end = split_boundary::<P>(xml, path, boundary)?;
    let mut container = xml.lens_mut_at::<P>(path)?;
    if has_range {
        container
            .insert_at_with_ns::<CommentRangeEnd>(end, W)?
            .set_attr(W_ID, id)?;
    }
    let reference = end + usize::from(has_range);
    let mut run = container.insert_at_with_ns::<Run>(reference, W)?;
    run.insert::<RunProperties>()?
        .insert::<RunStyle>()?
        .set_attr(w("val"), style)?;
    run.insert::<CommentReference>()?.set_attr(W_ID, id)?;
    Ok(())
}

fn put_anchor_start<P: ElementSpec>(
    xml: &mut XmlDocument,
    path: &[usize],
    boundary: Boundary,
    id: &str,
) -> Result<()>
where
    Run: ChildOf<P>,
    CommentRangeStart: ChildOf<P>,
{
    let start = split_boundary::<P>(xml, path, boundary)?;
    xml.lens_mut_at::<P>(path)?
        .insert_at_with_ns::<CommentRangeStart>(start, W)?
        .set_attr(W_ID, id)?;
    Ok(())
}

fn can_hold_comment_anchor(node: &RawNode) -> bool {
    node.is(Paragraph::QNAME) || node.is(Hyperlink::QNAME)
}

fn append_ex(xml: &mut XmlDocument, para: &str, parent: Option<&str>, done: bool) -> Result<()> {
    require_ignorable(xml.root(), &[], NS_W15)?;
    let mut root = xml.lens_mut::<CommentsEx>()?;
    let mut entry = root.insert::<CommentEx>()?;
    entry.set_attr(EX_PARA, para)?;
    entry.set_attr(DONE, if done { "1" } else { "0" })?;
    if let Some(parent) = parent {
        entry.set_attr(EX_PARENT, parent)?;
    }
    Ok(())
}

fn make_addressable(
    comments: &mut XmlDocument,
    def: Definition<'_>,
    used: &mut BTreeSet<u32>,
) -> Result<String> {
    let (relative, last) = comment_paragraphs(def.node)
        .last()
        .ok_or(Error::ParentNotAddressable)?;
    if def.para.is_some() {
        return Ok(last.attr(PARA).expect("parsed paraId").to_owned());
    }
    let path: Vec<_> = std::iter::once(def.index).chain(relative).collect();
    require_ignorable(comments.root(), &path, NS_W14).map_err(|_| Error::ParentNotAddressable)?;
    let para = random_hex(used)?;
    let mut p = comments.lens_mut_at::<Paragraph>(&path)?;
    p.set_attr(PARA, &para)?;
    p.set_attr(TEXT, "77777777")?;
    Ok(para)
}

fn check_revision(root: &RawNode, path: &[usize]) -> Result<()> {
    for depth in 0..path.len() {
        let parent = node_at(root, &path[..depth]);
        let context = match parent.name().local() {
            "ins" => Some(RunContext::Inserted),
            "del" => Some(RunContext::Deleted),
            "moveFrom" => Some(RunContext::MovedFrom),
            "moveTo" => Some(RunContext::MovedTo),
            _ => None,
        };
        if let Some(context) = context
            && parent.name().ns() == NS_W
        {
            return Err(Error::InsideRevision { context });
        }
    }
    Ok(())
}

fn check_alternate_content(root: &RawNode, path: &[usize]) -> Result<()> {
    if (0..path.len())
        .any(|depth| node_at(root, &path[..depth]).is(QName::new(MC_NS, "AlternateContent")))
    {
        return Err(Error::UnsupportedContext {
            ancestor: "mc:AlternateContent",
        });
    }
    Ok(())
}

fn anchor_paths(
    root: &RawNode,
    ids: &BTreeSet<CommentId>,
) -> impl Iterator<Item = (Vec<usize>, usize, CommentId)> {
    walk(root).filter_map(|(path, node)| {
        let id = id(node)?;
        (ids.contains(&id)).then_some(())?;
        Some((path, anchor_kind(node)?, id))
    })
}

fn remove_at(xml: &mut XmlDocument, path: &[usize]) -> Result<()> {
    let (&index, parent) = path.split_last().ok_or(Error::InvalidCommentPart)?;
    let node = node_at(xml.root(), parent);
    macro_rules! remove {
        ($($ty:ty),+ $(,)?) => {
            $(if node.is(<$ty>::QNAME) {
                xml.lens_mut_at::<$ty>(parent)?.remove_at(index);
                return Ok(());
            })+
        };
    }
    remove!(
        Run,
        Paragraph,
        Body,
        Table,
        TableRow,
        TableCell,
        Header,
        Footer,
        Footnote,
        Endnote,
        Comment,
        Comments,
        CommentsEx,
        CommentsIds,
        CommentsExtensible,
        Hyperlink,
        SdtContentRun,
        SimpleField
    );
    Err(Error::UnsupportedContext {
        ancestor: "comment marker parent",
    })
}

impl Document {
    fn range_boundary(
        &self,
        at: RunRef,
        offset: Option<usize>,
        start: bool,
    ) -> Result<(Vec<usize>, Boundary, usize)> {
        let run = self.run_at(at).ok_or(Error::NoSuchRun {
            paragraph: at.para().index(),
            run: at.index(),
        })?;
        let index = &self.stories[at.para().story().get()].index;
        let mut path = index.path(run.info.frame);
        let root = self.story_root_node(at.para().story());
        check_alternate_content(root, &path)?;
        if (0..path.len()).any(|depth| node_at(root, &path[..depth]).is(w("txbxContent"))) {
            return Err(Error::UnsupportedContext {
                ancestor: "w:txbxContent",
            });
        }
        run.check_settable()?;
        let child = path.pop().ok_or(Error::InvalidTextRange)?;
        if !can_hold_comment_anchor(node_at(root, &path)) {
            return Err(Error::InvalidTextRange);
        }
        let text = run.text();
        let byte = offset.unwrap_or(if start { 0 } else { text.len() });
        if !text.is_char_boundary(byte)
            || (offset.is_some() && (start && byte == text.len() || !start && byte == 0))
        {
            return Err(Error::InvalidTextRange);
        }
        let boundary = if byte == 0 && start {
            Boundary::Between(child)
        } else if byte == text.len() {
            Boundary::Between(child + 1)
        } else {
            Boundary::Inside { child, byte }
        };
        Ok((path, boundary, byte))
    }

    /// Adds a comment and its anchors in one package transaction.
    pub fn add_comment(&mut self, range: TextRange, comment: NewComment) -> Result<CommentId> {
        self.comment_write_gate()?;
        let story = range.first.para().story();
        if story != range.last.para().story()
            || self.stories.get(story.get()).is_none()
            || (range.first.para().index(), range.first.index())
                > (range.last.para().index(), range.last.index())
        {
            return Err(Error::InvalidTextRange);
        }
        let ancestor = match self.stories[story.get()].id.kind {
            StoryKind::Main => None,
            StoryKind::Header => Some("w:hdr"),
            StoryKind::Footer => Some("w:ftr"),
            StoryKind::Footnotes => Some("w:footnotes"),
            StoryKind::Endnotes => Some("w:endnotes"),
            StoryKind::Comments => Some("w:comments"),
        };
        if let Some(ancestor) = ancestor {
            return Err(Error::UnsupportedContext { ancestor });
        }
        let (start_path, start, from) = self.range_boundary(range.first, range.start_byte, true)?;
        let (end_path, end, to) = self.range_boundary(range.last, range.end_byte, false)?;
        if range.first == range.last && from >= to {
            return Err(Error::InvalidTextRange);
        }
        let part = self.stories[story.get()].id.part.clone();
        self.insert_comment(
            Location {
                part,
                start: Some((start_path, start)),
                end: (end_path, end),
                has_range: true,
            },
            comment,
            None,
        )
    }

    /// Replies to a top-level comment, using its range or point anchor.
    pub fn reply(&mut self, parent: CommentId, comment: NewComment) -> Result<CommentId> {
        self.comment_write_gate()?;
        let parts = self.comment_parts()?;
        let extended = self.load_extended(&parts, false)?;
        let comments = self.load_comments(&parts)?;
        let defs = definitions(comments.root(), extended.as_ref().map(XmlDocument::root));
        let parent_def = definition(&defs, parent)?;
        if parent_def.parent_para.is_some() {
            return Err(Error::NestedReply);
        }
        let location = self.reply_location(parent, &thread(&defs, parent_def))?;
        self.insert_comment(location, comment, Some(parent))
    }

    fn reply_location(&self, parent: CommentId, ids: &BTreeSet<CommentId>) -> Result<Location> {
        let mut location = None;
        for story in self
            .stories()
            .filter(|story| story.id().kind() != StoryKind::Comments)
        {
            let anchors = anchor_paths(story.raw(), ids).collect::<Vec<_>>();
            for (path, _, _) in &anchors {
                check_alternate_content(story.raw(), path)?;
            }
            if location.is_some() {
                continue;
            }
            let Some((parent_ref, _, _)) = anchors
                .iter()
                .find(|(_, kind, id)| *kind == 2 && *id == parent)
            else {
                continue;
            };
            let (reference, _, _) = anchors
                .iter()
                .rev()
                .find(|(_, kind, _)| *kind == 2)
                .expect("parent reference already found");
            check_revision(story.raw(), reference)?;
            let reference_run = &reference[..reference.len() - 1];
            let (&reference_index, end_path) = reference_run
                .split_last()
                .ok_or(Error::InvalidCommentPart)?;
            if !node_at(story.raw(), reference_run).is(Run::QNAME)
                || !can_hold_comment_anchor(node_at(story.raw(), end_path))
            {
                return Err(Error::InvalidCommentPart);
            }

            let start = if let Some((path, _, _)) = anchors
                .iter()
                .find(|(_, kind, id)| *kind == 0 && *id == parent)
            {
                if !anchors
                    .iter()
                    .any(|(_, kind, id)| *kind == 1 && *id == parent)
                    || path >= parent_ref
                {
                    return Err(Error::InvalidCommentPart);
                }
                check_revision(story.raw(), path)?;
                let (&start_index, start_path) =
                    path.split_last().ok_or(Error::InvalidCommentPart)?;
                let paragraph = node_at(story.raw(), start_path);
                if !can_hold_comment_anchor(paragraph) {
                    return Err(Error::UnsupportedContext {
                        ancestor: "a block-level comment range",
                    });
                }
                let mut after = start_index + 1;
                for (index, node) in paragraph
                    .indexed_elements()
                    .filter(|(index, _)| *index > start_index)
                {
                    if !node.is(CommentRangeStart::QNAME)
                        || !id(node).is_some_and(|id| ids.contains(&id))
                    {
                        break;
                    }
                    after = index + 1;
                }
                Some((start_path.to_vec(), Boundary::Between(after)))
            } else {
                if anchors
                    .iter()
                    .any(|(_, kind, id)| *kind == 1 && *id == parent)
                {
                    return Err(Error::InvalidCommentPart);
                }
                None
            };
            location = Some(Location {
                part: story.id().part().to_owned(),
                has_range: start.is_some(),
                start,
                end: (end_path.to_vec(), Boundary::Between(reference_index + 1)),
            });
        }
        location.ok_or(Error::InvalidCommentPart)
    }

    fn insert_comment(
        &mut self,
        location: Location,
        comment: NewComment,
        parent: Option<CommentId>,
    ) -> Result<CommentId> {
        let parts = self.comment_parts()?;
        let mut comments = self.load_comments(&parts)?;
        let mut extended = self.load_extended(&parts, parent.is_none())?;
        let (high, mut para_ids, mut durable_ids) = self.comment_ids(&parts)?;
        let id = CommentId(
            high.checked_add(1)
                .and_then(|id| i32::try_from(id).ok())
                .ok_or(Error::AnnotationIdsExhausted)?,
        );
        let parent = if let Some(parent) = parent {
            let original = comments.clone();
            let defs = definitions(original.root(), extended.as_ref().map(XmlDocument::root));
            Some(make_addressable(
                &mut comments,
                definition(&defs, parent)?,
                &mut para_ids,
            )?)
        } else {
            None
        };

        if comment.paragraphs.is_empty() {
            return Err(Error::InvalidValue {
                what: "comment paragraphs",
                value: String::new(),
            });
        }
        let (local, utc) = dates(comment.date)?;
        let pieces = comment
            .paragraphs
            .iter()
            .map(|text| map_text(text))
            .collect::<Result<Vec<_>>>()?;
        let declare_ignorable = require_ignorable(comments.root(), &[], NS_W14).is_err();

        if self.styles_ambiguous {
            return Err(Error::AmbiguousStylesPart);
        }
        let mut styles = match &self.styles_part {
            Some(part) => self.pkg.xml_part(part)?.clone(),
            None => self.new_wml_xml("styles")?,
        };
        let mut used = self.used_style_ids()?;
        let (body_style, body_added) = named_annotation_style(
            &mut styles,
            &mut used,
            "annotation text",
            "CommentText",
            "paragraph",
            Some("20"),
        )?;
        let (ref_style, ref_added) = named_annotation_style(
            &mut styles,
            &mut used,
            "annotation reference",
            "CommentReference",
            "character",
            Some("16"),
        )?;

        let mut root = comments.lens_mut::<Comments>()?;
        let mut new = root.insert_with_ns::<Comment>(W)?;
        new.set_attr(W_ID, id.0.format_into(&mut NumBuffer::new()))?;
        new.set_attr(w("author"), &comment.author)?;
        new.set_attr(w("date"), &local)?;
        if let Some(initials) = &comment.initials {
            new.set_attr(w("initials"), initials)?;
        }
        let mut last = String::new();
        for (i, pieces) in pieces.iter().enumerate() {
            last = random_hex(&mut para_ids)?;
            let declarations = if declare_ignorable {
                COMMENT_PARAGRAPH_NS
            } else {
                &[]
            };
            let mut paragraph = new.insert_with_ns::<Paragraph>(declarations)?;
            paragraph.set_attr(PARA, &last)?;
            paragraph.set_attr(TEXT, "77777777")?;
            if declare_ignorable {
                let prefix = paragraph
                    .node()
                    .attrs()
                    .iter()
                    .find(|attr| attr.name().is(PARA))
                    .and_then(|attr| attr.name().prefix())
                    .expect("a namespaced attribute uses a prefix")
                    .to_owned();
                paragraph.set_attr(IGNORABLE, &prefix)?;
            }
            paragraph
                .insert::<ParagraphProperties>()?
                .insert::<ParagraphStyleId>()?
                .set_attr(w("val"), &body_style)?;
            if i == 0 {
                styled_run(&mut paragraph, None, &ref_style)?
                    .insert::<AnnotationReferenceMark>()?;
            }
            write_pieces(&mut paragraph.insert::<Run>()?, pieces)?;
        }
        if let Some(ex) = &mut extended {
            append_ex(ex, &last, parent.as_deref(), false)?;
        }

        let mut story = self.pkg.xml_part(&location.part)?.clone();
        put_anchor(&mut story, &location, id, &ref_style)?;
        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, &location.part, story)?;
        self.stage_companions(
            &mut plan,
            &parts,
            &last,
            &mut durable_ids,
            &comment.author,
            &utc,
        )?;
        let comments_part = self.stage_wml_part(&mut plan, "comments", parts.comments, comments)?;
        if let Some(ex) = extended {
            self.stage_word_part(&mut plan, "commentsExtended", parts.extended, ex, EX_REL)?;
        }
        let styles_part = if body_added || ref_added {
            Some(self.stage_wml_part(&mut plan, "styles", self.styles_part.clone(), styles)?)
        } else {
            self.styles_part.clone()
        };
        self.pkg.apply(plan)?;
        self.styles_part = styles_part;
        self.annotation_high_water = id.0.cast_unsigned();
        self.refresh_comment_indexes(&comments_part, &location.changed_paths());
        Ok(id)
    }

    fn stage_companions(
        &mut self,
        plan: &mut RegistryPlan,
        parts: &Parts,
        para: &str,
        used: &mut BTreeSet<u32>,
        author: &str,
        utc: &str,
    ) -> Result<()> {
        if let Some(name) = &parts.ids
            && self.pkg.xml_part(name)?.root().is(CommentsIds::QNAME)
        {
            let durable = random_hex(used)?;
            let mut xml = self.pkg.xml_part(name)?.clone();
            require_ignorable(xml.root(), &[], NS_W16CID)?;
            let mut root = xml.lens_mut::<CommentsIds>()?;
            let mut entry = root.insert::<schema::CommentId>()?;
            entry.set_attr(CID_PARA, para)?;
            entry.set_attr(CID_DURABLE, &durable)?;
            plan.stage_part(&self.pkg, name, xml)?;

            if let Some(name) = &parts.extensible {
                let mut xml = self.pkg.xml_part(name)?.clone();
                if xml.root().is(CommentsExtensible::QNAME) {
                    require_ignorable(xml.root(), &[], NS_W16CEX)?;
                    let mut root = xml.lens_mut::<CommentsExtensible>()?;
                    let mut entry = root.insert::<CommentExtensible>()?;
                    entry.set_attr(CEX_DURABLE, &durable)?;
                    entry.set_attr(QName::new(NS_W16CEX, "dateUtc"), utc)?;
                    plan.stage_part(&self.pkg, name, xml)?;
                }
            }
        }
        if let Some(name) = &parts.people {
            let mut xml = self.pkg.xml_part(name)?.clone();
            let author_attr = QName::new(NS_W15, "author");
            if xml.root().is(People::QNAME)
                && !xml
                    .root()
                    .elements()
                    .any(|n| n.is(Person::QNAME) && n.attr(author_attr) == Some(author))
            {
                require_ignorable(xml.root(), &[], NS_W15)?;
                let mut root = xml.lens_mut::<People>()?;
                let mut person = root.insert::<Person>()?;
                person.set_attr(author_attr, author)?;
                let mut presence = person.insert::<PresenceInfo>()?;
                presence.set_attr(QName::new(NS_W15, "providerId"), "None")?;
                presence.set_attr(QName::new(NS_W15, "userId"), author)?;
                plan.stage_part(&self.pkg, name, xml)?;
            }
        }
        Ok(())
    }

    fn refresh_comment_indexes(&mut self, comments: &str, affected: &ChangedPaths) {
        self.refresh_note_story(StoryKind::Comments, comments.to_owned());
        for i in 0..self.stories.len() {
            let story = crate::StoryIndex(i);
            let data = &self.stories[i];
            if data.id.part == comments {
                // Removing a definition can shift all later paragraph indices.
                for index in 0..data.index.paragraphs.len() {
                    self.bump(ParaRef::new(story, index));
                }
            } else if let Some(paths) = affected.get(&data.id.part) {
                let changed = data
                    .index
                    .paragraphs
                    .iter()
                    .enumerate()
                    .filter_map(|(index, para)| {
                        let path = data.index.path(para.frame);
                        paths
                            .iter()
                            .any(|changed| changed.starts_with(&path))
                            .then_some(index)
                    })
                    .collect::<Vec<_>>();
                self.reindex(story);
                for index in changed {
                    self.bump(ParaRef::new(story, index));
                }
            }
        }
    }
}

fn remove_entries(
    xml: &mut XmlDocument,
    element: QName,
    attr: QName,
    ids: &BTreeSet<u32>,
) -> Result<()> {
    let indexes = xml
        .root()
        .indexed_elements()
        .filter_map(|(i, node)| {
            (node.is(element) && hex(node.attr(attr)).is_some_and(|id| ids.contains(&id)))
                .then_some(i)
        })
        .collect::<Vec<_>>();
    for i in indexes.into_iter().rev() {
        remove_at(xml, &[i])?;
    }
    Ok(())
}

impl Document {
    /// Sets the resolution state of the whole thread containing the given comment.
    pub fn set_resolved(&mut self, thread_id: CommentId, done: bool) -> Result<()> {
        self.comment_write_gate()?;
        let parts = self.comment_parts()?;
        let Some(part) = &parts.comments else {
            return Err(Error::NoSuchComment(thread_id.0));
        };
        let original = self.load_comments(&parts)?;
        let mut comments = original.clone();
        let mut extended = self.load_extended(&parts, false)?.expect("legacy refused");
        let before = extended.clone();
        let defs = definitions(original.root(), Some(before.root()));
        let thread = thread(&defs, thread_root(&defs, thread_id)?);
        let (_, mut para_ids, _) = self.comment_ids(&parts)?;
        for id in thread {
            let para = make_addressable(&mut comments, definition(&defs, id)?, &mut para_ids)?;
            let value = hex(Some(&para));
            let matches = before
                .root()
                .indexed_elements()
                .filter(|(_, node)| node.is(CommentEx::QNAME) && hex(node.attr(EX_PARA)) == value)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                append_ex(&mut extended, &para, None, done)?;
            } else {
                for (index, entry) in matches {
                    if entry
                        .attr(DONE)
                        .and_then(ooxml_core::value::parse_on_off)
                        .unwrap_or(false)
                        != done
                    {
                        extended
                            .lens_mut_at::<CommentEx>(&[index])?
                            .set_attr(DONE, if done { "1" } else { "0" })?;
                    }
                }
            }
        }

        let mut plan = RegistryPlan::new(&self.pkg);
        let refresh = comments.is_dirty() && comments.serialize() != original.serialize();
        if refresh {
            plan.stage_part(&self.pkg, part, comments)?;
        }
        if parts.extended.is_none() || extended.serialize() != before.serialize() {
            self.stage_word_part(
                &mut plan,
                "commentsExtended",
                parts.extended,
                extended,
                EX_REL,
            )?;
        }
        self.pkg.apply(plan)?;
        if refresh {
            self.refresh_comment_indexes(part, &ChangedPaths::new());
        }
        Ok(())
    }

    /// Deletes a comment, its descendants, and the corresponding story markers.
    pub fn delete_comment(&mut self, id: CommentId) -> Result<()> {
        self.comment_write_gate()?;
        let parts = self.comment_parts()?;
        let mut extended = self.load_extended(&parts, false)?.expect("legacy refused");
        let Some(comments_part) = &parts.comments else {
            return Err(Error::NoSuchComment(id.0));
        };
        let original = self.load_comments(&parts)?;
        let defs = definitions(original.root(), Some(extended.root()));
        let ids = thread(&defs, definition(&defs, id)?);
        let (high, _, _) = self.comment_ids(&parts)?;
        let definitions = ids
            .iter()
            .map(|id| definition(&defs, *id))
            .collect::<Result<Vec<_>>>()?;
        let paras = definitions
            .iter()
            .filter_map(|d| d.para)
            .collect::<BTreeSet<_>>();
        if defs
            .iter()
            .any(|d| !ids.contains(&d.id) && d.para.is_some_and(|p| paras.contains(&p)))
        {
            return Err(Error::InvalidCommentPart);
        }

        let mut edits = BTreeMap::new();
        let mut affected = ChangedPaths::new();
        for part in self.registry_parts_with_glossary(&[], false)? {
            let original = self.pkg.xml_part(&part)?;
            let anchors = anchor_paths(original.root(), &ids).collect::<Vec<_>>();
            if anchors.is_empty() {
                continue;
            }
            affected.insert(
                part.clone(),
                anchors.iter().map(|(path, _, _)| path.clone()).collect(),
            );
            let mut xml = original.clone();
            for (path, kind, _) in anchors.into_iter().rev() {
                check_revision(original.root(), &path)?;
                remove_at(&mut xml, &path)?;
                if kind == 2 {
                    let run = &path[..path.len() - 1];
                    let node = node_at(xml.root(), run);
                    if node.is(Run::QNAME)
                        && node.children().iter().all(|item| match item {
                            RawItem::Element(el) => el.is(RunProperties::QNAME),
                            RawItem::Text(text) => text.is_whitespace(),
                            RawItem::Markup(_) => false,
                        })
                    {
                        remove_at(&mut xml, run)?;
                    }
                }
            }
            edits.insert(part, xml);
        }
        let mut comments = edits
            .remove(comments_part)
            .unwrap_or_else(|| original.clone());
        let mut indexes = definitions.iter().map(|d| d.index).collect::<Vec<_>>();
        indexes.sort_unstable();
        for i in indexes.into_iter().rev() {
            remove_at(&mut comments, &[i])?;
        }

        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, comments_part, comments)?;
        if let Some(part) = &parts.extended {
            remove_entries(&mut extended, CommentEx::QNAME, EX_PARA, &paras)?;
            if extended.serialize() != self.pkg.xml_part(part)?.serialize() {
                plan.stage_part(&self.pkg, part, extended)?;
            }
        }
        if let Some(part) = &parts.ids {
            let original = self.pkg.xml_part(part)?.clone();
            if original.root().is(CommentsIds::QNAME) {
                let mut xml = original.clone();
                let durable = original
                    .root()
                    .elements()
                    .filter(|node| {
                        node.is(schema::CommentId::QNAME)
                            && hex(node.attr(CID_PARA)).is_some_and(|p| paras.contains(&p))
                    })
                    .filter_map(|node| hex(node.attr(CID_DURABLE)))
                    .collect::<BTreeSet<_>>();
                remove_entries(&mut xml, schema::CommentId::QNAME, CID_PARA, &paras)?;
                if xml.serialize() != original.serialize() {
                    plan.stage_part(&self.pkg, part, xml)?;
                }
                if let Some(part) = &parts.extensible {
                    let original = self.pkg.xml_part(part)?.clone();
                    if original.root().is(CommentsExtensible::QNAME) {
                        let mut xml = original.clone();
                        remove_entries(&mut xml, CommentExtensible::QNAME, CEX_DURABLE, &durable)?;
                        if xml.serialize() != original.serialize() {
                            plan.stage_part(&self.pkg, part, xml)?;
                        }
                    }
                }
            }
        }
        for (part, xml) in edits {
            plan.stage_part(&self.pkg, &part, xml)?;
        }
        self.pkg.apply(plan)?;
        self.annotation_high_water = high;
        self.refresh_comment_indexes(comments_part, &affected);
        Ok(())
    }
}
