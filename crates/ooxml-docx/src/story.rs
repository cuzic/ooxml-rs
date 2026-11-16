//! Stories (ADR-0007 §2): the main document, headers, footers, footnotes,
//! endnotes and comments, and the references that address their
//! paragraphs, runs and sections.

use std::sync::atomic::{AtomicU64, Ordering};

use ooxml_core::RawNode;

use crate::body::BodyIndex;

/// What kind of part a story is (ADR-0007 §2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum StoryKind {
    /// The main document part's `w:body`.
    Main,
    /// A header part (`w:hdr`).
    Header,
    /// A footer part (`w:ftr`).
    Footer,
    /// The footnotes part (`w:footnotes`).
    Footnotes,
    /// The endnotes part (`w:endnotes`).
    Endnotes,
    /// The comments part (`w:comments`).
    Comments,
}

/// Identifies a story: its kind and its part name. Stable across saves.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StoryId {
    pub(crate) kind: StoryKind,
    pub(crate) part: String,
}

impl StoryId {
    /// The story's kind.
    pub fn kind(&self) -> StoryKind {
        self.kind
    }

    /// The story's part name (e.g. `word/header1.xml`).
    pub fn part(&self) -> &str {
        &self.part
    }
}

/// Position of a story in [`crate::Document::stories`]; stable for the
/// lifetime of the [`crate::Document`]. The main story is always index 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StoryIndex(pub(crate) usize);

impl StoryIndex {
    /// The main story.
    pub const MAIN: StoryIndex = StoryIndex(0);

    /// The index as a number.
    pub fn get(self) -> usize {
        self.0
    }
}

/// A paragraph of a story: the story and the paragraph's index in
/// [`crate::Story::paragraphs`]. Stable for the lifetime of the
/// [`crate::Document`] (Phase1 only appends paragraphs, at the end of the
/// main story).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParaRef {
    pub(crate) story: StoryIndex,
    pub(crate) index: usize,
}

impl ParaRef {
    /// A reference to paragraph `index` of `story` (checked on use).
    pub fn new(story: StoryIndex, index: usize) -> Self {
        ParaRef { story, index }
    }

    /// The story.
    pub fn story(self) -> StoryIndex {
        self.story
    }

    /// The paragraph's index in its story.
    pub fn index(self) -> usize {
        self.index
    }
}

/// A run: its paragraph and its index in [`crate::Paragraph::runs`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunRef {
    pub(crate) para: ParaRef,
    pub(crate) run: usize,
}

impl RunRef {
    /// A reference to run `run` of `para` (checked on use).
    pub fn new(para: ParaRef, run: usize) -> Self {
        RunRef { para, run }
    }

    /// The paragraph.
    pub fn para(self) -> ParaRef {
        self.para
    }

    /// The run's index in its paragraph.
    pub fn index(self) -> usize {
        self.run
    }
}

/// What holds a paragraph.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Container {
    /// The main story's body or a header or footer (tables, content
    /// controls and custom XML included).
    Body,
    /// A footnote or endnote, with its `w:id` as written.
    Note {
        /// [`StoryKind::Footnotes`] or [`StoryKind::Endnotes`].
        kind: StoryKind,
        /// The note's `w:id`.
        id: Option<String>,
        /// A separator, continuation separator or continuation notice
        /// (ADR-0007 §2.2): its runs are never settable.
        separator: bool,
    },
    /// A comment, with its `w:id` as written.
    Comment {
        /// The comment's `w:id`.
        id: Option<String>,
    },
}

/// A header or footer reference type (`w:type` of `w:headerReference` /
/// `w:footerReference`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeaderFooterType {
    /// `default` (also when `w:type` is absent).
    Default,
    /// `first`: shown on a section's first page when it has `w:titlePg`.
    First,
    /// `even`: shown on even pages when `settings/w:evenAndOddHeaders` is on.
    Even,
}

/// How a section uses a header or footer part (ADR-0007 §2.3), read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SectionUse {
    /// The section's index in document order.
    pub section: usize,
    /// The reference type through which the section uses the part, after
    /// inheritance from earlier sections.
    pub kind: HeaderFooterType,
    /// Whether Word shows it *(u)*: `false` for a `first` part in a
    /// section without `w:titlePg`, and for an `even` part when
    /// `settings/w:evenAndOddHeaders` is off.
    pub displayed: bool,
}

/// A process-unique identifier of an opened [`crate::Document`], carried
/// by every [`crate::Match`] (ADR-0007 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocumentId(u64);

impl DocumentId {
    pub(crate) fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        DocumentId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A story's read model.
#[derive(Debug)]
pub(crate) struct StoryData {
    pub id: StoryId,
    pub index: BodyIndex,
    pub sections: Vec<SectionUse>,
}

/// A story of a [`crate::Document`].
#[derive(Clone, Copy, Debug)]
pub struct Story<'d> {
    pub(crate) doc: &'d crate::Document,
    pub(crate) index: StoryIndex,
    pub(crate) data: &'d StoryData,
    pub(crate) root: &'d RawNode,
}

impl<'d> Story<'d> {
    /// The story's identity.
    pub fn id(&self) -> &'d StoryId {
        &self.data.id
    }

    /// Its position in [`crate::Document::stories`].
    pub fn index(&self) -> StoryIndex {
        self.index
    }

    /// The story part's root element (`w:document`, `w:hdr`, …).
    pub fn raw(&self) -> &'d RawNode {
        self.root
    }

    /// Number of paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.data.index.paragraphs.len()
    }

    /// The story's paragraphs in document order (the walk of ADR-0004
    /// §3.1, per note or comment for notes and comments parts).
    pub fn paragraphs(&self) -> impl Iterator<Item = crate::Paragraph<'d>> + use<'d> {
        let (doc, story, data, root) = (self.doc, self.index, self.data, self.root);
        (0..data.index.paragraphs.len()).map(move |i| crate::Paragraph::new(doc, story, data, root, i))
    }

    /// Paragraph `index`.
    pub fn paragraph(&self, index: usize) -> Option<crate::Paragraph<'d>> {
        (index < self.data.index.paragraphs.len())
            .then(|| crate::Paragraph::new(self.doc, self.index, self.data, self.root, index))
    }

    /// For a header or footer: every section that uses the part, and how
    /// (ADR-0007 §2.3). Empty for other stories, and for a header or footer
    /// no section references.
    pub fn sections(&self) -> &'d [SectionUse] {
        &self.data.sections
    }
}
