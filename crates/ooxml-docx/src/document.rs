//! [`Document`], [`Paragraph`], [`Run`] (ADR-0004 §3).

use std::path::Path;
use std::sync::Arc;

use ooxml_core::name::{STRICT_NS_PREFIX, XML_SPACE};
use ooxml_core::opc::TargetMode;
use ooxml_core::{ChildOf, ElementSpec, LensMut, Package, RawNode, XmlDocument};
use ooxml_schema::wml::{
    self, Body, Break, DocumentProtectionValues, Hyperlink, Paragraph as P, Run as R, Settings, SettingsView, TabChar,
    Table, TableCell, TableRow, Text,
};

use crate::RunContext;
use crate::body::{self, BodyIndex, ParaInfo, RunInfo, Wrapper};
use crate::error::{Error, Result};
use crate::fields::{self, FieldScan};
use crate::names::{SETTINGS_RELATIONSHIP, STORY_RELATIONSHIPS, is};
use crate::text::{Piece, check_text_only, current_pieces, map_text, needs_preserve, run_text};

/// A document-level write gate (ADR-0004 §6, W1–W3), computed at open.
#[derive(Clone, Debug)]
enum Gate {
    TrackRevisionsOn,
    DocumentProtected(String),
    StaleFieldRisk(Vec<String>),
}

impl Gate {
    fn to_error(&self) -> Error {
        match self {
            Gate::TrackRevisionsOn => Error::TrackRevisionsOn,
            Gate::DocumentProtected(edit) => Error::DocumentProtected { edit: edit.clone() },
            Gate::StaleFieldRisk(codes) => Error::StaleFieldRisk { codes: codes.clone() },
        }
    }
}

/// An existing Word document opened for lossless editing.
///
/// The main document part is parsed once at open (Layer 2); `settings.xml`
/// is parsed read-only and stays a raw copy on save; headers, footers,
/// notes and comments are parsed transiently to scan their fields; every
/// other part is passed through untouched (Layer 1).
#[derive(Debug)]
pub struct Document {
    pkg: Package,
    main: String,
    gates: Vec<Gate>,
    update_fields: Option<bool>,
    index: BodyIndex,
}

impl Document {
    /// Opens a `.docx`, `.docm`, `.dotx` or `.dotm` file.
    ///
    /// Errors (ADR-0004 §6): O1 `Core(EncryptedPackage)`, O2
    /// `Core(DtdNotAllowed)`, O3 `Core(StrictNotSupported)`, O4
    /// [`Error::MainPartMissing`], `Core(WrongElement)` if the main part is
    /// not a `w:document`, and I/O, zip or XML errors. The write gates (W1–W3)
    /// do not fail the open; see [`Self::write_gate`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path).map_err(ooxml_core::Error::from)?)
    }

    /// Opens a document from bytes. See [`Self::open`].
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self> {
        let mut pkg = Package::from_bytes(bytes)?;
        let main = pkg.main_document_part().filter(|m| pkg.contains(m)).ok_or(Error::MainPartMissing)?;
        let xml = pkg.xml_part(&main)?;
        let ns = xml.root().name().ns();
        if ns.starts_with(STRICT_NS_PREFIX) {
            return Err(ooxml_core::Error::StrictNotSupported(ns.into()).into());
        }
        xml.lens::<wml::Document>()?;
        let index = body::index(xml.root());
        let mut codes = FieldScan::scan(xml.root());

        let rels = pkg.relationships(&main)?;
        let target = |rel_type: &str| -> Vec<String> {
            rels.by_type(rel_type)
                .filter(|r| r.target_mode == TargetMode::Internal)
                .filter_map(|r| rels.resolve(r))
                .filter(|n| pkg.contains(n))
                .collect()
        };
        let settings = target(SETTINGS_RELATIONSHIP).into_iter().next();
        let stories: Vec<String> = STORY_RELATIONSHIPS.iter().flat_map(|t| target(t)).collect();
        for story in stories {
            let doc = XmlDocument::parse(&pkg.read_part(&story)?)?;
            codes.extend(FieldScan::scan(doc.root()));
        }

        let mut gates = Vec::new();
        let mut update_fields = None;
        if let Some(name) = settings {
            let view = SettingsView(pkg.xml_part(&name)?.lens::<Settings>()?);
            // An invalid ST_OnOff value fails closed: the gate is on.
            if view.track_revisions().is_some_and(|t| t.val().unwrap_or(true)) {
                gates.push(Gate::TrackRevisionsOn);
            }
            if let Some(p) = view.document_protection() {
                let enforced = p.enforcement().unwrap_or(Some(true)).unwrap_or(false);
                let edit = p.edit();
                let restricted = match &edit {
                    None => false,
                    Some(e) => e.known() != Some(DocumentProtectionValues::None),
                };
                if enforced && restricted {
                    gates.push(Gate::DocumentProtected(edit.map(|e| e.as_str().to_owned()).unwrap_or_default()));
                }
            }
            update_fields = view.update_fields_on_open().map(|u| u.val().unwrap_or(true));
        }
        let refused = fields::refused(codes);
        if !refused.is_empty() {
            gates.push(Gate::StaleFieldRisk(refused));
        }
        Ok(Document { pkg, main, gates, update_fields, index })
    }

    /// Serializes the package: every part except an edited main part is
    /// copied with its original compressed bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.pkg.to_bytes()?)
    }

    /// Saves atomically (temporary file + rename); saving over the source
    /// path is safe.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        Ok(self.pkg.save(path)?)
    }

    /// The underlying package (read-only), e.g. to read a part's current
    /// bytes with [`Package::read_part`].
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// Name of the main document part (found by relationship, usually
    /// `word/document.xml`).
    pub fn main_part_name(&self) -> &str {
        &self.main
    }

    /// `Ok(())` if writes are allowed; otherwise the first document-level
    /// gate, in the order W1 [`Error::TrackRevisionsOn`], W3
    /// [`Error::DocumentProtected`], W2 [`Error::StaleFieldRisk`]. Reads
    /// work either way.
    pub fn write_gate(&self) -> Result<()> {
        self.gates.first().map_or(Ok(()), |g| Err(g.to_error()))
    }

    /// Every document-level gate that applies (empty if writes are allowed).
    pub fn write_gates(&self) -> Vec<Error> {
        self.gates.iter().map(Gate::to_error).collect()
    }

    /// `settings/w:updateFields` as read (`None` if absent). Phase0 reports
    /// it and never writes it.
    pub fn update_fields_on_open(&self) -> Option<bool> {
        self.update_fields
    }

    fn root(&self) -> &RawNode {
        self.pkg.parsed_part(&self.main).expect("the main part is parsed at open and never replaced").root()
    }

    /// Number of paragraphs in body flow.
    pub fn paragraph_count(&self) -> usize {
        self.index.paragraphs.len()
    }

    /// Paragraphs of the body flow in document order: `w:body`, descending
    /// into tables, content controls and custom XML, but not into text
    /// boxes, drawings or `mc:AlternateContent`.
    pub fn paragraphs(&self) -> impl Iterator<Item = Paragraph<'_>> {
        let root = self.root();
        self.index.paragraphs.iter().enumerate().map(move |(index, info)| Paragraph { root, info, index })
    }

    /// Paragraph `index` (as in [`Self::paragraphs`]).
    pub fn paragraph(&self, index: usize) -> Option<Paragraph<'_>> {
        self.index.paragraphs.get(index).map(|info| Paragraph { root: self.root(), info, index })
    }

    fn run(&self, paragraph: usize, run: usize) -> Result<Run<'_>> {
        self.paragraph(paragraph).ok_or(Error::NoSuchParagraph(paragraph))?.run(run).ok_or(Error::NoSuchRun { paragraph, run })
    }

    /// Replaces the content of run `run` of paragraph `paragraph` with
    /// `text` (ADR-0004 §3): `\t` → `w:tab`, `\n`/`\r\n`/`\r` → `w:br`, the
    /// rest → `w:t` (with `xml:space="preserve"` where needed). `w:rPr` and
    /// all attributes are kept; a `w:lastRenderedPageBreak` in the run is
    /// dropped. Writing content the run already has is a no-op.
    ///
    /// Errors, before any change: [`Self::write_gate`],
    /// [`Run::check_settable`], [`Error::InvalidXmlChar`],
    /// [`Error::NoSuchParagraph`], [`Error::NoSuchRun`].
    pub fn set_run_text(&mut self, paragraph: usize, run: usize, text: &str) -> Result<()> {
        self.write_gate()?;
        let r = self.run(paragraph, run)?;
        r.check_settable()?;
        let pieces = map_text(text)?;
        if current_pieces(r.node).as_ref() == Some(&pieces) {
            return Ok(());
        }
        let path = r.info.path.clone();
        let xml = self.pkg.xml_part_mut(&self.main)?;
        let mut lens = run_lens(xml.lens_mut::<wml::Document>()?, &path).ok_or(Error::NoSuchRun { paragraph, run })?;
        write_pieces(&mut lens, &pieces)
    }

    /// Appends a paragraph with `text` as the last block of the body,
    /// before the body-level `w:sectPr` (or at the end of the body if there
    /// is none). It has no `w:pPr`, `w:rPr` or IDs; empty `text` gives an
    /// empty paragraph. Returns the new paragraph's index.
    ///
    /// Errors, before any change: [`Self::write_gate`],
    /// [`Error::InvalidXmlChar`], [`Error::NoBody`], [`Error::InsideField`]
    /// (the body ends inside an unterminated field), A1
    /// `Core(UnresolvedSibling)` (the body has an `mc:AlternateContent`
    /// child).
    pub fn append_paragraph(&mut self, text: &str) -> Result<usize> {
        self.write_gate()?;
        let pieces = map_text(text)?;
        let b = self.index.body.ok_or(Error::NoBody)?;
        if self.index.open_fields_at_end > 0 {
            return Err(Error::InsideField);
        }
        let xml = self.pkg.xml_part_mut(&self.main)?;
        let mut body = xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(b).ok_or(Error::NoBody)?;
        let mut p = body.insert::<P>()?;
        if !pieces.is_empty() {
            write_pieces(&mut p.insert::<R>()?, &pieces)?;
        }
        self.index = body::index(self.root());
        Ok(self.index.paragraphs.len() - 1)
    }
}

/// Mutable lens on the run at `path` (from the part root). Only paths
/// through `w:body`, `w:tbl`/`w:tr`/`w:tc`, `w:p` and `w:hyperlink` are
/// followed; those are the only ones a settable run has.
fn run_lens<'a>(doc: LensMut<'a, wml::Document>, path: &[usize]) -> Option<LensMut<'a, R>> {
    let (&b, rest) = path.split_first()?;
    let (p, rest) = to_paragraph(doc.into_child_at::<Body>(b)?, rest)?;
    to_run(p, rest)
}

fn to_paragraph<'a, 'p, E>(c: LensMut<'a, E>, path: &'p [usize]) -> Option<(LensMut<'a, P>, &'p [usize])>
where
    E: ElementSpec,
    P: ChildOf<E>,
    Table: ChildOf<E>,
{
    let (&i, rest) = path.split_first()?;
    if c.node().children().get(i)?.as_element().is_some_and(is::<P>) {
        return Some((c.into_child_at::<P>(i)?, rest));
    }
    let [r, k, rest @ ..] = rest else { return None };
    let tc = c.into_child_at::<Table>(i)?.into_child_at::<TableRow>(*r)?.into_child_at::<TableCell>(*k)?;
    to_paragraph::<TableCell>(tc, rest)
}

fn to_run<'a, E>(c: LensMut<'a, E>, path: &[usize]) -> Option<LensMut<'a, R>>
where
    E: ElementSpec,
    R: ChildOf<E>,
    Hyperlink: ChildOf<E>,
{
    match path {
        [i] => c.into_child_at::<R>(*i),
        [i, rest @ ..] => to_run::<Hyperlink>(c.into_child_at::<Hyperlink>(*i)?, rest),
        [] => None,
    }
}

/// Replaces the run's content (everything but `w:rPr` and whitespace-only
/// text) with `pieces`. Fast path: one `w:t` replaced by one `w:t` keeps
/// the element and its open tag.
fn write_pieces(run: &mut LensMut<'_, R>, pieces: &[Piece]) -> Result<()> {
    let content: Vec<usize> = run.node().indexed_elements().filter(|(_, c)| !is::<wml::RunProperties>(c)).map(|(i, _)| i).collect();
    if let ([i], [Piece::Text(s)]) = (&content[..], pieces)
        && run.node().children()[*i].as_element().is_some_and(is::<Text>)
    {
        let mut t = run.child_mut::<Text>().expect("checked above");
        return set_t(&mut t, s);
    }
    for &i in content.iter().rev() {
        run.remove_at(i);
    }
    for piece in pieces {
        match piece {
            Piece::Text(s) => set_t(&mut run.insert::<Text>()?, s)?,
            Piece::Tab => drop(run.insert::<TabChar>()?),
            Piece::Br => drop(run.insert::<Break>()?),
        }
    }
    Ok(())
}

fn set_t(t: &mut LensMut<'_, Text>, s: &str) -> Result<()> {
    if needs_preserve(s) {
        t.set_attr(XML_SPACE, "preserve")?;
    }
    t.set_text(s)?;
    Ok(())
}

fn node_at<'a>(root: &'a RawNode, path: &[usize]) -> &'a RawNode {
    path.iter().fold(root, |n, &i| n.children()[i].as_element().expect("indexed paths point at elements"))
}

/// A paragraph of the body flow.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'d> {
    root: &'d RawNode,
    info: &'d ParaInfo,
    index: usize,
}

impl<'d> Paragraph<'d> {
    /// Index in [`Document::paragraphs`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// The `w:p` element.
    pub fn raw(&self) -> &'d RawNode {
        node_at(self.root, &self.info.path)
    }

    /// The "all revisions accepted" text: the text of [`Self::runs`] except
    /// runs inside `w:del` or `w:moveFrom`. Field results are included,
    /// field instructions are not (they are `w:instrText`).
    pub fn text(&self) -> String {
        self.runs().filter(|r| !r.info.wrappers.iter().any(|w| w.is_removed_text())).map(|r| r.text()).collect()
    }

    /// Number of runs.
    pub fn run_count(&self) -> usize {
        self.info.runs.len()
    }

    /// Every run of the paragraph's flow, in document order: direct runs
    /// and runs inside `w:hyperlink`, revisions (including deleted ones),
    /// smart tags, content controls, custom XML, simple fields, `w:dir`,
    /// `w:bdo` and ruby base text.
    pub fn runs(&self) -> impl Iterator<Item = Run<'d>> + use<'d> {
        let root = self.root;
        self.info.runs.iter().enumerate().map(move |(index, info)| Run { node: node_at(root, &info.path), info, index })
    }

    /// Run `index` (as in [`Self::runs`]).
    pub fn run(&self, index: usize) -> Option<Run<'d>> {
        self.info.runs.get(index).map(|info| Run { node: node_at(self.root, &info.path), info, index })
    }
}

/// A run (`w:r`) in a paragraph's flow.
#[derive(Clone, Copy, Debug)]
pub struct Run<'d> {
    node: &'d RawNode,
    info: &'d RunInfo,
    index: usize,
}

impl<'d> Run<'d> {
    /// Index in [`Paragraph::runs`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// The `w:r` element.
    pub fn raw(&self) -> &'d RawNode {
        self.node
    }

    /// The run's text: `w:t` (and `w:delText`), `w:tab` → `\t`,
    /// `w:br`/`w:cr` → `\n`, `w:noBreakHyphen` → U+2011, `w:softHyphen` →
    /// U+00AD.
    pub fn text(&self) -> String {
        run_text(self.node)
    }

    /// Where the run sits: the outermost wrapper other than `w:hyperlink`
    /// between `w:body` and the run, else [`RunContext::Hyperlink`] if it is
    /// in a hyperlink, else [`RunContext::Direct`].
    pub fn context(&self) -> RunContext {
        let ws = &self.info.wrappers;
        ws.iter().find(|w| **w != Wrapper::Hyperlink).map_or_else(
            || if ws.is_empty() { RunContext::Direct } else { RunContext::Hyperlink },
            |w| w.context(),
        )
    }

    /// Is the run inside a complex field (instruction or result), or does it
    /// hold a `w:fldChar`?
    pub fn in_field(&self) -> bool {
        self.info.in_field
    }

    /// Why [`Document::set_run_text`] would refuse this run, without trying
    /// (the document-level gates are not included). Checked in order:
    /// R1 [`Error::InsideField`], R2 [`Error::InsideRevision`], R3
    /// [`Error::UnsupportedContext`], R4 [`Error::RunNotTextOnly`].
    pub fn check_settable(&self) -> Result<()> {
        let ws = &self.info.wrappers;
        if self.info.in_field || ws.contains(&Wrapper::SimpleField) {
            return Err(Error::InsideField);
        }
        if let Some(w) = ws.iter().find(|w| w.is_revision()) {
            return Err(Error::InsideRevision { context: w.context() });
        }
        if let Some(w) = ws.iter().find(|w| **w != Wrapper::Hyperlink) {
            return Err(Error::UnsupportedContext { ancestor: w.qname() });
        }
        check_text_only(self.node)
    }
}
