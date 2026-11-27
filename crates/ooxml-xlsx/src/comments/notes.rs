use std::collections::{HashMap, VecDeque};

use ooxml_core::{ChildOf, Content, ElementSpec, QName, RawNode, XmlDocument};
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

/// How a legacy note box follows cells in the ExcelJS model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteEditAs {
    /// Absolute positioning.
    #[default]
    Absolute,
    /// One-cell positioning.
    OneCells,
    /// Two-cell positioning.
    TwoCells,
}

/// VML box options for a legacy note.
#[derive(Clone, Debug)]
pub struct NoteBox {
    /// The textbox uses custom margins when true.
    pub custom_inset: bool,
    /// Four textbox insets in centimetres.
    pub inset: [f64; 4],
    /// Whether the note box is locked.
    pub locked: bool,
    /// Whether the note text is locked.
    pub lock_text: bool,
    /// How the note box follows cells.
    pub edit_as: NoteEditAs,
}

impl NoteBox {
    pub(super) fn validate(&self) -> Result<()> {
        if self.inset.iter().all(|value| value.is_finite() && *value >= 0.0 && (value * 10.0).is_finite()) {
            return Ok(());
        }
        Err(Error::InvalidSetting { what: "note box inset".into(), reason: "expected finite nonnegative margins".into() })
    }
}

/// Options for a legacy cell note.
#[derive(Clone, Debug)]
pub struct Note {
    pub(super) text: String,
    runs: Option<Vec<crate::RichTextRun>>,
    pub(super) author: String,
    pub(super) visible: bool,
    pub(super) box_settings: Option<NoteBox>,
}

impl Note {
    /// Creates a hidden note.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            runs: None,
            author: String::new(),
            visible: false,
            box_settings: None,
        }
    }

    /// Creates a hidden note with formatted text runs.
    pub fn rich_text(runs: Vec<crate::RichTextRun>) -> Self {
        Self { runs: Some(runs), ..Self::new("") }
    }

    /// Applies explicit VML box options when the note is created.
    pub fn box_settings(mut self, settings: NoteBox) -> Self {
        self.box_settings = Some(settings);
        self
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
    source: &'doc str,
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
            source: doc.source(),
        })
    }

    pub(super) fn positions(&self) -> impl Iterator<Item = CellIndex> + '_ {
        self.entries.iter().map(|(at, _)| *at)
    }

    pub(super) fn entries(&self, row: Option<u32>) -> impl Iterator<Item = (CellIndex, String, String)> + '_ {
        let authors: Vec<_> = self.authors.elements().filter(|node| node.is(AUTHOR)).collect();
        self.entries.iter().filter(move |(at, _)| row.is_none_or(|row| at.row == row)).map(move |(at, note)| {
            let author = note.attr(attr("authorId"))
                .and_then(|value| value.parse::<usize>().ok())
                .and_then(|index| authors.get(index))
                .map(|author| escape::decode(&author.text()).into_owned())
                .unwrap_or_default();
            (*at, author, text(note))
        })
    }

    /// Every note with its stored `text` markup and, for run text, the run
    /// fonts as written (`None` when a run font cannot be read).
    pub(super) fn sources(&self) -> impl Iterator<Item = (CellIndex, super::NoteSource)> + '_ {
        self.entries(None).zip(&self.entries).map(|((at, author, text), (_, note))| {
            let body = note.elements().find(|node| node.is(TEXT));
            let text_xml = body.and_then(|body| {
                let span = body.span()?;
                Some(match span.close {
                    Some((close, _)) => self.source.get(span.open_end..close)?.to_owned(),
                    None => String::new(),
                })
            });
            let runs = body
                .filter(|body| body.elements().any(|node| node.is(QName::new(SML, "r"))))
                .and_then(|body| crate::registry::read::declared_rich_text_runs(body).ok());
            (at, super::NoteSource { at, author, text, text_xml, runs })
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
        self.append_all(at, std::slice::from_ref(note), uid)
    }

    pub(super) fn append_all(&self, at: CellIndex, notes: &[Note], uid: Option<&str>) -> Result<Vec<Patch>> {
        if self.at(at)?.is_some() {
            return Err(Error::NoteExists);
        }
        self.write_at(at, notes, uid, Vec::new())
    }

    pub(super) fn replace_all(&self, at: CellIndex, notes: &[Note]) -> Result<(Vec<Patch>, bool)> {
        let existing: Vec<_> = self.entries.iter()
            .filter_map(|(address, node)| (*address == at).then_some(*node))
            .collect();
        let had_notes = !existing.is_empty();
        Ok((self.write_at(at, notes, None, existing)?, had_notes))
    }

    fn write_at(&self, at: CellIndex, notes: &[Note], uid: Option<&str>, existing: Vec<&RawNode>) -> Result<Vec<Patch>> {
        let mut authors: Vec<_> = self.authors.elements().map(|node| node.text().into_owned()).collect();
        let mut reusable = HashMap::<_, HashMap<_, VecDeque<_>>>::new();
        for node in &existing {
            if let Some(author) = node.attr(attr("authorId"))
                .and_then(|value| value.parse::<usize>().ok())
                .and_then(|index| authors.get(index))
            {
                reusable.entry(author.clone()).or_default()
                    .entry(text(node)).or_default().push_back(*node);
            }
        }
        let mut author_markup = String::new();
        let mut markup = String::new();
        let author_ns = self.namespace_declaration(self.authors);
        let note_ns = self.namespace_declaration(self.list);
        let xr = uid.map(|id| self.revision_attrs(id)).transpose()?.unwrap_or_default();
        for note in notes {
            let encoded = escape::encode(&note.author);
            if note.runs.is_none()
                && let Some(node) = reusable.get_mut(encoded.as_ref())
                    .and_then(|by_text| by_text.get_mut(&note.text))
                    .and_then(VecDeque::pop_front)
            {
                let span = node.span().expect("parsed comment");
                markup.push_str(&self.source[span.start..span.end()]);
                continue;
            }
            let author = match authors.iter().position(|author| author == encoded.as_ref()) {
                Some(author) => author,
                None => {
                    let value = xml::text(&encoded);
                    author_markup.push_str(&format!("<author{author_ns}>{value}</author>"));
                    authors.push(encoded.into_owned());
                    authors.len() - 1
                }
            };
            let text = match &note.runs {
                Some(runs) => formatted_text(runs)?,
                None => rich_text(&note.text, ""),
            };
            markup.push_str(&format!(
                "<comment{note_ns} ref=\"{at}\" authorId=\"{author}\" shapeId=\"0\"{xr}>{text}</comment>"
            ));
        }
        let mut edits = Vec::new();
        if !author_markup.is_empty() {
            edits.push(xml::insert(self.authors, None, author_markup));
        }
        if let Some((first, rest)) = existing.split_first() {
            edits.push(xml::replace(first, markup));
            edits.extend(rest.iter().map(|node| xml::replace(node, String::new())));
            return Ok(edits);
        }
        if markup.is_empty() {
            return Ok(edits);
        }
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
        self.replace_text(note, |ns| Ok(rich_text(text, ns)))
    }

    pub(super) fn set_runs(&self, note: &RawNode, runs: &[crate::RichTextRun]) -> Result<Patch> {
        self.replace_text(note, |_| formatted_text(runs))
    }

    fn replace_text(&self, note: &RawNode, markup: impl FnOnce(&str) -> Result<String>) -> Result<Patch> {
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
        Ok(xml::replace(existing, markup(&ns)?))
    }
}

pub(super) fn text(note: &RawNode) -> String {
    const T: QName = QName::new(SML, "t");
    const R: QName = QName::new(SML, "r");
    let mut value = String::new();
    if let Some(text) = note.elements().find(|node| node.is(TEXT)) {
        for node in text.elements() {
            let content = if node.is(T) {
                Some(node)
            } else if node.is(R) {
                node.elements().find(|child| child.is(T))
            } else {
                None
            };
            if let Some(content) = content {
                value.push_str(&escape::decode(&content.text()));
            }
        }
    }
    value
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

enum NoteText {}
impl ElementSpec for NoteText {
    const QNAME: QName = TEXT;
    const SDK_NAME: &'static str = "NoteText";
    const ATTRS: &'static [ooxml_core::AttrSpec] = sml::CommentText::ATTRS;
    const CONTENT: Content = sml::CommentText::CONTENT;
}
impl ChildOf<NoteText> for sml::Run {}

fn formatted_text(runs: &[crate::RichTextRun]) -> Result<String> {
    use crate::registry::strings::{RichRun, build_run};
    if runs.is_empty() {
        return Err(Error::InvalidSetting { what: "note text".into(), reason: "expected text runs".into() });
    }
    let mut doc = XmlDocument::parse(format!(r#"<text xmlns="{SML}"/>"#).as_bytes())?;
    let mut text = doc.lens_mut::<NoteText>()?;
    for (value, font) in runs {
        if let Some(font) = font {
            crate::Format::with_font(font.clone()).validate()?;
        }
        build_run(&mut text, &RichRun { font: font.clone(), escaped: escape::encode(value).into_owned() })?;
    }
    Ok(doc.serialize())
}

// A fallback is paired with an existing thread by its revision UID.
pub(super) fn is_fallback(node: &RawNode, thread_id: &str) -> bool {
    node.attr(UID)
        .is_some_and(|uid| uid.eq_ignore_ascii_case(thread_id))
}
