//! The body walk: paragraphs and runs of the main document's body flow, in
//! document order, with their wrappers and complex-field state (ADR-0004 §3).
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
    Body, DeletedRun, FieldChar, Hyperlink, InsertedRun, MoveFromRun, MoveToRun, Paragraph, Run, SimpleField, Table,
    TableCell, TableRow,
};

use crate::RunContext;
use crate::names::{BDO, CUSTOM_XML, DIR, RUBY, RUBY_BASE, SDT, SDT_CONTENT, SMART_TAG, is};

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

/// A run: its path from the part root, its wrappers (outermost first,
/// paragraph-level wrappers included) and whether it is inside a complex
/// field.
#[derive(Clone, Debug)]
pub(crate) struct RunInfo {
    pub path: Vec<usize>,
    pub wrappers: Vec<Wrapper>,
    pub in_field: bool,
}

/// A paragraph in body flow.
#[derive(Clone, Debug)]
pub(crate) struct ParaInfo {
    pub path: Vec<usize>,
    pub runs: Vec<RunInfo>,
}

/// Result of the walk.
#[derive(Clone, Debug, Default)]
pub(crate) struct BodyIndex {
    /// Child index of `w:body` under `w:document`.
    pub body: Option<usize>,
    pub paragraphs: Vec<ParaInfo>,
    /// Complex fields still open after the last run of the body.
    pub open_fields_at_end: usize,
}

#[derive(Default)]
struct Walker {
    path: Vec<usize>,
    wrappers: Vec<Wrapper>,
    depth: usize,
    paragraphs: Vec<ParaInfo>,
}

/// Walks the body of a `w:document` root.
pub(crate) fn index(root: &RawNode) -> BodyIndex {
    let Some((b, body)) = root.indexed_elements().find(|(_, n)| is::<Body>(n)) else {
        return BodyIndex::default();
    };
    let mut w = Walker { path: vec![b], ..Walker::default() };
    w.block(body);
    BodyIndex { body: Some(b), paragraphs: w.paragraphs, open_fields_at_end: w.depth }
}

impl Walker {
    fn with<T>(&mut self, index: usize, wrapper: Option<Wrapper>, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(index);
        if let Some(wr) = wrapper {
            self.wrappers.push(wr);
        }
        let out = f(self);
        if wrapper.is_some() {
            self.wrappers.pop();
        }
        self.path.pop();
        out
    }

    /// Block-level content: body, cells, block/row/cell SDTs and custom XML.
    fn block(&mut self, node: &RawNode) {
        for (i, child) in node.indexed_elements() {
            if is::<Paragraph>(child) {
                self.with(i, None, |w| w.paragraph(child));
            } else if is::<Table>(child) || is::<TableRow>(child) || is::<TableCell>(child) || child.is(SDT_CONTENT) {
                self.with(i, None, |w| w.block(child));
            } else if child.is(SDT) || child.is(CUSTOM_XML) {
                self.with(i, Wrapper::of(child), |w| w.block(child));
            }
        }
    }

    fn paragraph(&mut self, node: &RawNode) {
        let mut runs = Vec::new();
        self.inline(node, &mut runs);
        self.paragraphs.push(ParaInfo { path: self.path.clone(), runs });
    }

    /// Paragraph-level content.
    fn inline(&mut self, node: &RawNode, runs: &mut Vec<RunInfo>) {
        for (i, child) in node.indexed_elements() {
            if is::<Run>(child) {
                self.with(i, None, |w| w.run(child, runs));
            } else if child.is(SDT_CONTENT) {
                self.with(i, None, |w| w.inline(child, runs));
            } else if let Some(wr) = Wrapper::of(child) {
                self.with(i, Some(wr), |w| w.inline(child, runs));
            }
        }
    }

    fn run(&mut self, node: &RawNode, runs: &mut Vec<RunInfo>) {
        let before = self.depth;
        let mut has_fld_char = false;
        for child in node.elements().filter(|c| is::<FieldChar>(c)) {
            has_fld_char = true;
            match child.attr(FieldChar::ATTR_W_FLD_CHAR_TYPE) {
                Some("begin") => self.depth += 1,
                Some("end") => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
        runs.push(RunInfo {
            path: self.path.clone(),
            wrappers: self.wrappers.clone(),
            in_field: before > 0 || self.depth > 0 || has_fld_char,
        });
        for (j, ruby) in node.indexed_elements().filter(|(_, c)| c.is(RUBY)) {
            for (k, base) in ruby.indexed_elements().filter(|(_, c)| c.is(RUBY_BASE)) {
                self.with(j, Some(Wrapper::Ruby), |w| w.with(k, None, |w| w.inline(base, runs)));
            }
        }
    }
}
