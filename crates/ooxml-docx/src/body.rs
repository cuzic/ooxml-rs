//! The story walk: paragraphs and runs of a story's flow, in document order,
//! with their wrappers and complex-field state (ADR-0004 §3, ADR-0007 §2.1).
//! A story is the main document's body, a header or footer part, or the
//! notes or comments of a notes or comments part.
//!
//! Body flow means: `w:body`, descending into `w:tbl`/`w:tr`/`w:tc`,
//! `w:sdt`/`w:sdtContent` and `w:customXml` (block, row and cell level),
//! then inside each `w:p` into `w:hyperlink`, `w:ins`, `w:del`,
//! `w:moveFrom`, `w:moveTo`, `w:smartTag`, `w:sdt`/`w:sdtContent`,
//! `w:customXml`, `w:fldSimple`, `w:dir`, `w:bdo` and
//! `w:r/w:ruby/w:rubyBase`. It does **not** descend into drawings, VML,
//! OLE objects, `mc:AlternateContent` or math: text boxes are separate
//! stories.
//!
//! The complex-field depth is one counter for the whole body, so a field
//! that spans paragraphs, cells or wrappers is tracked (review round 1, m2).
//! An unterminated `begin` keeps the rest of the body inside the field
//! (fail closed).

use ooxml_core::RawNode;
use ooxml_schema::wml::{
    Body, Comment, DeletedRun, Endnote, FieldChar, Footnote, Hyperlink, InsertedRun, MoveFromRun, MoveToRun, Paragraph, Run, SimpleField, Table,
    TableCell, TableRow,
};

use crate::RunContext;
use crate::names::{W_ID, W_TYPE, BDO, CUSTOM_XML, DIR, RUBY, RUBY_BASE, SDT, SDT_CONTENT, SMART_TAG, is};

/// An element between `w:body` and a run (other than `w:p`, `w:tbl`,
/// `w:tr`, `w:tc`, `w:sdtContent`, `w:rubyBase`, which add nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wrapper {
    Hyperlink,
    Ins,
    Del,
    MoveFrom,
    MoveTo,
    SmartTag,
    Sdt,
    CustomXml,
    SimpleField,
    Dir,
    Bdo,
    Ruby,
}

impl Wrapper {
    fn of(node: &RawNode) -> Option<Self> {
        Some(match () {
            () if is::<Hyperlink>(node) => Self::Hyperlink,
            () if is::<InsertedRun>(node) => Self::Ins,
            () if is::<DeletedRun>(node) => Self::Del,
            () if is::<MoveFromRun>(node) => Self::MoveFrom,
            () if is::<MoveToRun>(node) => Self::MoveTo,
            () if node.is(SMART_TAG) => Self::SmartTag,
            () if node.is(SDT) => Self::Sdt,
            () if node.is(CUSTOM_XML) => Self::CustomXml,
            () if is::<SimpleField>(node) => Self::SimpleField,
            () if node.is(DIR) => Self::Dir,
            () if node.is(BDO) => Self::Bdo,
            _ => return None,
        })
    }

    pub(crate) fn qname(self) -> &'static str {
        match self {
            Self::Hyperlink => "w:hyperlink",
            Self::Ins => "w:ins",
            Self::Del => "w:del",
            Self::MoveFrom => "w:moveFrom",
            Self::MoveTo => "w:moveTo",
            Self::SmartTag => "w:smartTag",
            Self::Sdt => "w:sdt",
            Self::CustomXml => "w:customXml",
            Self::SimpleField => "w:fldSimple",
            Self::Dir => "w:dir",
            Self::Bdo => "w:bdo",
            Self::Ruby => "w:ruby",
        }
    }

    pub(crate) fn context(self) -> RunContext {
        match self {
            Self::Hyperlink => RunContext::Hyperlink,
            Self::Ins => RunContext::Inserted,
            Self::Del => RunContext::Deleted,
            Self::MoveFrom => RunContext::MovedFrom,
            Self::MoveTo => RunContext::MovedTo,
            Self::SmartTag => RunContext::SmartTag,
            Self::Sdt => RunContext::ContentControl,
            Self::CustomXml => RunContext::CustomXml,
            Self::SimpleField => RunContext::SimpleField,
            Self::Dir | Self::Bdo => RunContext::Bidi,
            Self::Ruby => RunContext::Ruby,
        }
    }

    pub(crate) fn is_revision(self) -> bool {
        matches!(self, Self::Ins | Self::Del | Self::MoveFrom | Self::MoveTo)
    }

    /// Text a reader of the "all revisions accepted" view does not see.
    pub(crate) fn is_removed_text(self) -> bool {
        matches!(self, Self::Del | Self::MoveFrom)
    }
}

/// One element the walk descended into: its parent frame, its child index
/// in that parent, and the wrapper it adds. Runs and paragraphs refer to a
/// frame instead of storing their whole path, so the index is linear in the
/// number of visited elements whatever the nesting depth (review round 2,
/// N1: per-run paths made it quadratic in the depth of nested tables).
#[derive(Clone, Copy, Debug)]
struct Frame {
    parent: u32,
    index: u32,
    wrapper: Option<Wrapper>,
}

/// Parent of the `w:body` frame.
const NO_FRAME: u32 = u32::MAX;

/// A run: its frame, and whether it is inside a complex field.
#[derive(Clone, Debug)]
pub(crate) struct RunInfo {
    pub frame: u32,
    pub in_field: bool,
}

/// A paragraph in body flow.
#[derive(Clone, Debug)]
pub(crate) struct ParaInfo {
    pub frame: u32,
    pub runs: Vec<RunInfo>,
    /// A complex field was already open at the start of this paragraph.
    pub starts_in_field: bool,
    /// Index in [`BodyIndex::notes`] of the note or comment holding the
    /// paragraph (notes and comments parts only).
    pub note: Option<u32>,
}

/// A `w:footnote`, `w:endnote` or `w:comment` of a notes or comments part.
#[derive(Clone, Debug)]
pub(crate) struct NoteInfo {
    /// Its `w:id`, as written.
    pub id: Option<String>,
    /// A footnote or endnote whose `w:type` is `separator`,
    /// `continuationSeparator` or `continuationNotice` (ADR-0007 §2.2).
    pub separator: bool,
}

/// Result of the walk.
#[derive(Clone, Debug, Default)]
pub(crate) struct BodyIndex {
    /// Child index of `w:body` under `w:document`.
    pub body: Option<usize>,
    pub paragraphs: Vec<ParaInfo>,
    pub notes: Vec<NoteInfo>,
    /// Complex fields still open after the last run of the body.
    pub open_fields_at_end: usize,
    frames: Vec<Frame>,
}

impl BodyIndex {
    /// Child indices from the part root (`w:document`) to the element of
    /// `frame`.
    pub(crate) fn path(&self, frame: u32) -> Vec<usize> {
        let mut path = Vec::new();
        let mut f = frame;
        while f != NO_FRAME {
            let fr = self.frames[f as usize];
            path.push(fr.index as usize);
            f = fr.parent;
        }
        path.reverse();
        path
    }

    /// Wrappers between `w:body` and the element of `frame`, outermost
    /// first (paragraph-level wrappers included).
    pub(crate) fn wrappers(&self, frame: u32) -> Vec<Wrapper> {
        let mut out = Vec::new();
        let mut f = frame;
        while f != NO_FRAME {
            let fr = self.frames[f as usize];
            out.extend(fr.wrapper);
            f = fr.parent;
        }
        out.reverse();
        out
    }
}

/// How the children of an element on the walk stack are read. Shared
/// with the field scan ([`crate::fields`]), which must agree with the body
/// walk on which `w:fldChar` elements the walk counts (review round 3, m7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// `w:document`: only `w:body` is descended into.
    Document,
    /// `w:footnotes`, `w:endnotes`, `w:comments`: only their notes or
    /// comments are descended into (ADR-0007 §2.1).
    Notes,
    /// Body, cells, block/row/cell SDTs and custom XML.
    Block,
    /// Paragraph-level content.
    Inline,
    /// A run's children (only `w:ruby` is descended into; `w:fldChar` and
    /// `w:instrText` here are the ones the walk counts).
    Run,
    /// `w:ruby` (only `w:rubyBase`).
    Ruby,
}

/// The body walk's grammar: whether it descends into `child` of an element
/// read in `mode`, and if so, the mode for `child`'s children and the
/// wrapper `child` adds.
pub(crate) fn descend(mode: Mode, child: &RawNode) -> Option<(Mode, Option<Wrapper>)> {
    match mode {
        Mode::Document if is::<Body>(child) => Some((Mode::Block, None)),
        Mode::Notes if is_note(child) => Some((Mode::Block, None)),
        Mode::Block if is::<Paragraph>(child) => Some((Mode::Inline, None)),
        Mode::Block
            if is::<Table>(child) || is::<TableRow>(child) || is::<TableCell>(child) || child.is(SDT_CONTENT) =>
        {
            Some((Mode::Block, None))
        }
        Mode::Block if child.is(SDT) || child.is(CUSTOM_XML) => Some((Mode::Block, Wrapper::of(child))),
        Mode::Inline if is::<Run>(child) => Some((Mode::Run, None)),
        Mode::Inline if child.is(SDT_CONTENT) => Some((Mode::Inline, None)),
        Mode::Inline => Wrapper::of(child).map(|w| (Mode::Inline, Some(w))),
        Mode::Run if child.is(RUBY) => Some((Mode::Ruby, Some(Wrapper::Ruby))),
        Mode::Ruby if child.is(RUBY_BASE) => Some((Mode::Inline, None)),
        _ => None,
    }
}

/// `w:footnote`, `w:endnote` or `w:comment`.
fn is_note(node: &RawNode) -> bool {
    is::<Footnote>(node) || is::<Endnote>(node) || is::<Comment>(node)
}

/// Note types that are Word's note-area furniture (ADR-0007 §2.2).
const SEPARATOR_TYPES: &[&str] = &["separator", "continuationSeparator", "continuationNotice"];

/// The first element child of `node` at or after child index `from`.
pub(crate) fn next_element(node: &RawNode, from: usize) -> Option<(usize, &RawNode)> {
    node.children().get(from..)?.iter().enumerate().find_map(|(k, c)| c.as_element().map(|e| (from + k, e)))
}

/// Walks the main document's body (a `w:document` root).
pub(crate) fn index(root: &RawNode) -> BodyIndex {
    match root.indexed_elements().find(|(_, n)| is::<Body>(n)) {
        Some((b, body)) => {
            let mut out = walk(body, Some(b), Mode::Block);
            out.body = Some(b);
            out
        }
        None => BodyIndex::default(),
    }
}

/// Walks a story part's root (ADR-0007 §2.1): `w:hdr`/`w:ftr` hold
/// paragraphs directly (`Mode::Block`); notes and comments parts hold them
/// in each note or comment (`Mode::Notes`).
pub(crate) fn index_root(root: &RawNode, mode: Mode) -> BodyIndex {
    walk(root, None, mode)
}

/// Walks `top`, the child `top_index` of the part root (or the root itself
/// when `None`), whose children are read in `top_mode`. Iterative: the
/// depth of the part costs heap, not stack. The complex-field depth is one
/// counter for the whole story.
fn walk(top: &RawNode, top_index: Option<usize>, top_mode: Mode) -> BodyIndex {
    let mut out = BodyIndex::default();
    let new_frame = |out: &mut BodyIndex, parent: u32, index: usize, wrapper: Option<Wrapper>| -> u32 {
        let id = u32::try_from(out.frames.len()).expect("fewer than 2^32 elements");
        out.frames.push(Frame { parent, index: u32::try_from(index).expect("child index fits u32"), wrapper });
        id
    };
    let top_frame = match top_index {
        Some(i) => new_frame(&mut out, NO_FRAME, i, None),
        None => NO_FRAME,
    };
    let mut depth = 0usize;
    let mut note: Option<u32> = None;
    // (element, its frame, mode, index of the next child to look at)
    let mut stack: Vec<(&RawNode, u32, Mode, usize)> = vec![(top, top_frame, top_mode, 0)];
    while let Some((node, frame, mode, next)) = stack.last_mut() {
        let (node, frame, mode) = (*node, *frame, *mode);
        let Some((i, child)) = next_element(node, *next) else {
            stack.pop();
            continue;
        };
        *next = i + 1;
        let Some((child_mode, wrapper)) = descend(mode, child) else { continue };
        let f = new_frame(&mut out, frame, i, wrapper);
        if mode == Mode::Notes {
            let id = u32::try_from(out.notes.len()).expect("fewer than 2^32 notes");
            let separator = child.attr(W_TYPE).is_some_and(|t| SEPARATOR_TYPES.contains(&t));
            out.notes.push(NoteInfo { id: child.attr(W_ID).map(str::to_owned), separator });
            note = Some(id);
        }
        if mode == Mode::Block && is::<Paragraph>(child) {
            out.paragraphs.push(ParaInfo { frame: f, runs: Vec::new(), starts_in_field: depth > 0, note });
        }
        if child_mode == Mode::Run {
            let before = depth;
            // A run holding a `w:fldChar` is refused by R1 even when the
            // depth does not change (a lone `separate`); R4 refuses it as
            // well, so this is defence in depth (review round 3, m10).
            let mut has_fld_char = false;
            for c in child.elements().filter(|c| is::<FieldChar>(c)) {
                has_fld_char = true;
                match c.attr(FieldChar::ATTR_W_FLD_CHAR_TYPE) {
                    Some("begin") => depth += 1,
                    Some("end") => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            let para = out.paragraphs.last_mut().expect("runs are only walked inside a paragraph");
            para.runs.push(RunInfo { frame: f, in_field: before > 0 || depth > 0 || has_fld_char });
        }
        stack.push((child, f, child_mode, 0));
    }
    out.open_fields_at_end = depth;
    out
}
