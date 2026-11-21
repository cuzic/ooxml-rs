use ooxml_core::{ElementSpec, QName, RawNode, XmlDocument};
use ooxml_schema::sml;

use super::xml::{self, Patch, attr};
use crate::images::part_error;
use crate::{CellIndex, Error, Result, escape};

const SML: &str = crate::SML_NS;
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const XR: &str = "http://schemas.microsoft.com/office/spreadsheetml/2014/revision";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";
const AUTHORS: QName = sml::Authors::QNAME;
const AUTHOR: QName = sml::Author::QNAME;
const LIST: QName = sml::CommentList::QNAME;
const COMMENT: QName = sml::Comment::QNAME;
const TEXT: QName = sml::CommentText::QNAME;

pub(super) const ROOT: QName = sml::Comments::QNAME;
pub(super) const UID: QName = QName::new(XR, "uid");
pub(super) const EMPTY: &str = "<comments xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><authors/><commentList/></comments>";

/// Options for a legacy cell note.
#[derive(Clone, Debug)]
pub struct Note {
    pub(super) text: String,
    pub(super) author: String,
    pub(super) visible: bool,
}

impl Note {
    /// Creates a hidden note.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            author: String::new(),
            visible: false,
        }
    }

    /// Sets the note's author.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    /// Controls whether the note is visible when the worksheet opens.
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

pub(super) struct Notes<'doc> {
    root: &'doc RawNode,
    authors: &'doc RawNode,
    list: &'doc RawNode,
    entries: Vec<(CellIndex, &'doc RawNode)>,
    part: &'doc str,
}

impl<'doc> Notes<'doc> {
    pub(super) fn new(doc: &'doc XmlDocument, part: &'doc str) -> Result<Self> {
        let root = doc.root();
        xml::ordered(
            root,
            ROOT,
            &[AUTHORS, LIST, QName::new(SML, "extLst")],
            part,
        )?;
        let exactly_one = |name| {
            let mut nodes = root.elements().filter(|node| node.is(name));
            match (nodes.next(), nodes.next()) {
                (Some(node), None) => Ok(node),
                _ => Err(part_error(part, format!("expected one {}", name.local))),
            }
        };
        let authors = exactly_one(AUTHORS)?;
        let list = exactly_one(LIST)?;
        xml::ordered(authors, AUTHORS, &[AUTHOR], part)?;
        xml::ordered(list, LIST, &[COMMENT], part)?;
        let entries = list
            .elements()
            .map(|node| {
                let at = node
                    .attr(attr("ref"))
                    .and_then(|at| CellIndex::parse(at).ok())
                    .ok_or_else(|| part_error(part, "invalid note reference"))?;
                Ok((at, node))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            root,
            authors,
            list,
            entries,
            part,
        })
    }

    pub(super) fn at(&self, at: CellIndex) -> Result<Option<&'doc RawNode>> {
        let mut matches = self.entries.iter().filter(|(cell, _)| *cell == at);
        let first = matches.next().map(|(_, node)| *node);
        if matches.next().is_some() {
            return Err(part_error(self.part, "multiple notes match the cell"));
        }
        Ok(first)
    }

    pub(super) fn append(
        &self,
        at: CellIndex,
        note: &Note,
        uid: Option<&str>,
    ) -> Result<Vec<Patch>> {
        if self.at(at)?.is_some() {
            return Err(Error::NoteExists);
        }
        let mut edits = Vec::new();
        let encoded = escape::encode(&note.author);
        let author = self
            .authors
            .elements()
            .position(|node| node.text() == encoded);
        let author = author.unwrap_or_else(|| {
            let value = xml::text(&encoded);
            let ns = self.namespace_declaration(self.authors);
            let markup = format!("<author{ns}>{value}</author>");
            edits.push(xml::insert(self.authors, None, markup));
            self.authors.elements().count()
        });
        let xr = uid
            .map(|id| self.revision_attrs(id))
            .transpose()?
            .unwrap_or_default();
        let text = rich_text(&note.text, "");
        let ns = self.namespace_declaration(self.list);
        let markup = format!(
            "<comment{ns} ref=\"{at}\" authorId=\"{author}\" shapeId=\"0\"{xr}>{text}</comment>"
        );
        let before = self
            .entries
            .is_sorted_by_key(|(at, _)| (at.row, at.col))
            .then(|| {
                self.entries
                    .iter()
                    .find(|(cell, _)| (cell.row, cell.col) > (at.row, at.col))
            })
            .flatten()
            .map(|(_, node)| *node);
        edits.push(xml::insert(self.list, before, markup));
        Ok(edits)
    }

    fn namespace_declaration(&self, parent: &RawNode) -> String {
        xml::namespace_declaration(
            xml::default_namespace(parent).or_else(|| xml::default_namespace(self.root)),
            SML,
        )
    }

    fn revision_attrs(&self, id: &str) -> Result<String> {
        let namespace = |prefix| self.root.attr(QName::new(XMLNS, prefix));
        let xr = namespace("xr");
        let local_xr = self.list.attr(QName::new(XMLNS, "xr"));
        if local_xr.or(xr).is_some_and(|uri| uri != XR) {
            return Err(part_error(
                self.part,
                "xr prefix is bound to another namespace",
            ));
        }
        let ignorable = xr == Some(XR)
            && self
                .root
                .attr(QName::new(MC, "Ignorable"))
                .is_some_and(|value| value.split_ascii_whitespace().any(|prefix| prefix == "xr"));
        let declarations = if ignorable {
            String::new()
        } else {
            format!(" xmlns:mc=\"{MC}\" xmlns:xr=\"{XR}\" mc:Ignorable=\"xr\"")
        };
        Ok(format!("{declarations} xr:uid=\"{}\"", xml::attribute(id)))
    }

    pub(super) fn set_text(&self, note: &RawNode, text: &str) -> Result<Patch> {
        xml::ordered(
            note,
            COMMENT,
            &[TEXT, QName::new(SML, "commentPr")],
            self.part,
        )?;
        let mut matches = note.elements().filter(|node| node.is(TEXT));
        let existing = matches
            .next()
            .ok_or_else(|| part_error(self.part, "note has no text"))?;
        if matches.next().is_some() {
            return Err(part_error(self.part, "note has multiple text elements"));
        }
        let ns = xml::namespace_declaration(
            xml::default_namespace(note)
                .or_else(|| xml::default_namespace(self.list))
                .or_else(|| xml::default_namespace(self.root)),
            SML,
        );
        Ok(xml::replace(existing, rich_text(text, &ns)))
    }
}

fn rich_text(value: &str, ns: &str) -> String {
    let preserve = if value.starts_with(char::is_whitespace) || value.ends_with(char::is_whitespace)
    {
        " xml:space=\"preserve\""
    } else {
        ""
    };
    let encoded = xml::text(&escape::encode(value));
    format!("<text{ns}><r><t{preserve}>{encoded}</t></r></text>")
}

// A fallback is paired with an existing thread by its revision UID.
pub(super) fn is_fallback(node: &RawNode, thread_id: &str) -> bool {
    node.attr(UID)
        .is_some_and(|uid| uid.eq_ignore_ascii_case(thread_id))
}
