//! [`Document`], [`Paragraph`], [`Run`] (ADR-0004 §3, ADR-0007 §2).

use std::path::Path;
use std::sync::Arc;

use ooxml_core::name::{STRICT_NS_PREFIX, XML_SPACE};
use ooxml_core::opc::TargetMode;
use ooxml_core::{ChildOf, ElementSpec, LensMut, Package, QName, RawNode, XmlDocument};
use ooxml_schema::wml::{
    self, Body, Break, Comment, Comments, DocumentProtection, DocumentProtectionValues, DocumentProtectionView,
    Endnote, Endnotes, Footer, Footnote, Footnotes, Header, Hyperlink, Paragraph as P, ParagraphProperties, Run as R,
    SectionProperties, Settings, SettingsView, TabChar, Table, TableCell, TableRow, Text, TrackRevisions,
    TrackRevisionsView,
};

use crate::body::{self, BodyIndex, Mode, ParaInfo, RunInfo, Wrapper};
use crate::error::{Error, Result};
use crate::fields::{self, FieldScan, Operation};
use crate::names::{
    CUSTOM_PROPERTIES_NS, CUSTOM_PROPERTIES_RELATIONSHIP, EVEN_AND_ODD_HEADERS, FOOTER_REFERENCE, HEADER_REFERENCE,
    R_ID, RECOMMENDED, RPR_CHANGE, SETTINGS_RELATIONSHIP, STYLES_RELATIONSHIP, STORY_RELATIONSHIPS, TITLE_PG, VAL, W_TYPE,
    WRITE_PROTECTION, is,
};
use crate::story::{
    Container, DocumentId, HeaderFooterType, ParaRef, RunRef, SectionUse, Story, StoryData, StoryId, StoryIndex,
    StoryKind,
};
use crate::text::{Piece, check_text_only, current_pieces, lacks_needed_preserve, map_text, needs_preserve, run_text};
use crate::{RunContext, WriteProtection};

/// A document-level write gate (ADR-0004 §6, W1–W3), computed at open.
#[derive(Clone, Debug)]
enum Gate {
    TrackRevisionsOn,
    DocumentProtected(String),
    /// The refused field keys, for every operation.
    StaleFieldRisk(Vec<String>),
}

impl Gate {
    /// The error for `op`, or `None` when the gate does not apply to it
    /// (ADR-0007 §6.1: per-operation W2).
    fn error_for(&self, op: Operation) -> Option<Error> {
        match self {
            Gate::TrackRevisionsOn => Some(Error::TrackRevisionsOn),
            Gate::DocumentProtected(edit) => Some(Error::DocumentProtected { edit: edit.clone() }),
            Gate::StaleFieldRisk(codes) => {
                let codes: Vec<String> = codes.iter().filter(|c| !fields::allowed_for(op, c)).cloned().collect();
                (!codes.is_empty()).then_some(Error::StaleFieldRisk { codes })
            }
        }
    }
}

/// An existing Word document opened for lossless editing.
///
/// The main document part and every story part (headers, footers,
/// footnotes, endnotes, comments; ADR-0007 §2.1) are parsed once at open
/// (Layer 2) and kept; an unedited one is still written as a raw copy.
/// `settings.xml` is parsed read-only; every other part is passed through
/// untouched (Layer 1).
///
/// # Examples
///
/// ```no_run
/// use ooxml_docx::Document;
///
/// let mut doc = Document::open("letter.docx")?;
/// doc.write_gate()?; // track changes, protection, fields that could go stale
/// let hit = doc.paragraphs().find_map(|p| {
///     p.runs().find(|r| r.text() == "{{name}}" && r.check_settable().is_ok()).map(|r| (p.index(), r.index()))
/// });
/// if let Some((p, r)) = hit {
///     doc.set_run_text(p, r, "Ada Lovelace")?;
/// }
/// doc.append_paragraph("Regards,\tthe team")?;
/// doc.save("letter-filled.docx")?;
/// # Ok::<(), ooxml_docx::Error>(())
/// ```
///
/// A complete, runnable version is `examples/fill_runs.rs`.
#[derive(Debug)]
pub struct Document {
    pub(crate) pkg: Package,
    main: String,
    pub(crate) stories: Vec<StoryData>,
    /// Per story, per paragraph: the edit generation (ADR-0007 §7).
    pub(crate) generations: Vec<Vec<u64>>,
    pub(crate) id: DocumentId,
    /// Source of [`Self::generation`] values: every write gives its
    /// paragraphs a value never used before in this `Document`.
    gen_counter: u64,
    gates: Vec<Gate>,
    /// Parts named by the main part's settings relationships (ADR-0007 §6.3).
    pub(crate) settings_parts: Vec<String>,
    /// The part named by the main part's styles relationship (ADR-0007 §5).
    pub(crate) styles_part: Option<String>,
    /// More than one styles relationship: style setters refuse.
    pub(crate) styles_ambiguous: bool,
    pub(crate) annotation_high_water: u32,
    pub(crate) note_high_water: [i32; 2],
    pub(crate) theme_part: Option<String>,
    pub(crate) numbering_part: Option<String>,
    /// [`Document::enable_update_fields_on_open`] was called: W2 is lifted
    /// for every operation (ADR-0007 §6.3).
    pub(crate) fields_opt_in: bool,
    pub(crate) update_fields: Option<bool>,
    write_protection: Option<WriteProtection>,
    comment_protection: Option<String>,
    tracked_protection: Option<String>,
    control_value_protection: Option<String>,
    marked_final: bool,
}

/// Expected root of a story part.
fn story_root(kind: StoryKind) -> (QName, Mode) {
    match kind {
        StoryKind::Main => (wml::Document::QNAME, Mode::Document),
        StoryKind::Header => (Header::QNAME, Mode::Block),
        StoryKind::Footer => (Footer::QNAME, Mode::Block),
        StoryKind::Footnotes => (Footnotes::QNAME, Mode::Notes),
        StoryKind::Endnotes => (Endnotes::QNAME, Mode::Notes),
        StoryKind::Comments => (Comments::QNAME, Mode::Notes),
    }
}

const STORY_KINDS: [StoryKind; 5] =
    [StoryKind::Header, StoryKind::Footer, StoryKind::Footnotes, StoryKind::Endnotes, StoryKind::Comments];

impl Document {
    /// Opens a `.docx`, `.docm`, `.dotx` or `.dotm` file.
    ///
    /// Errors (ADR-0004 §6): O1 `Core(EncryptedPackage)`, O2
    /// `Core(DtdNotAllowed)`, O3 `Core(StrictNotSupported)`, O4
    /// [`Error::MainPartMissing`], O5 `Core(TooDeep)` (a parsed or scanned
    /// part nests elements deeper than
    /// [`ooxml_core::DEFAULT_MAX_DEPTH`]), `Core(WrongElement)` if the main
    /// part is not a `w:document`, and I/O, zip or XML errors. The write gates (W1–W3)
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
        let mut codes = FieldScan::scan(xml.root());
        let mut stories =
            vec![StoryData { id: StoryId { kind: StoryKind::Main, part: main.clone() }, index: body::index(xml.root()), sections: Vec::new() }];
        let mut bookmarks = std::collections::BTreeSet::new();
        fields::bookmark_names(xml.root(), &mut bookmarks);

        let rels = pkg.relationships(&main)?;
        let target = |rel_type: &str| -> Vec<String> {
            rels.by_type(rel_type)
                .filter(|r| r.target_mode == TargetMode::Internal)
                .filter_map(|r| rels.resolve(r))
                .filter(|n| pkg.contains(n))
                .collect()
        };
        // Every settings relationship (ECMA-376 allows one; with more, any
        // part that turns a gate on turns it on, review round 3, m11).
        let settings = target(SETTINGS_RELATIONSHIP);
        let settings_parts = settings.clone();
        // two styles relationships: the style operations fail closed (review of
        // PR #59, n3), as settings do with AmbiguousSettings
        let styles_targets = target(STYLES_RELATIONSHIP);
        let styles_ambiguous = styles_targets.len() > 1;
        let styles_part = styles_targets.into_iter().next();
        let story_parts: Vec<(StoryKind, String)> = STORY_KINDS
            .iter()
            .zip(STORY_RELATIONSHIPS)
            .flat_map(|(&k, t)| target(t).into_iter().map(move |n| (k, n)))
            .collect();
        let related = |kind| {
            let relation = format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}");
            target(&relation).into_iter().next()
        };
        let theme_part = related("theme");
        let numbering_part = related("numbering");
        let mut seen = std::collections::BTreeSet::new();
        for (kind, part) in story_parts {
            if !seen.insert(part.clone()) || part == main {
                continue;
            }
            let doc = pkg.xml_part(&part)?;
            codes.extend(FieldScan::scan(doc.root()));
            fields::bookmark_names(doc.root(), &mut bookmarks);
            // A part whose root is not the story's root element is scanned
            // (W2) but is not a story.
            let (root, mode) = story_root(kind);
            if doc.root().is(root) {
                let index = body::index_root(doc.root(), mode);
                stories.push(StoryData { id: StoryId { kind, part }, index, sections: Vec::new() });
            }
        }

        // Parsed once and kept (read-only, written raw). A styles part that
        // does not parse does not fail the open: the style operations report
        // the error (ADR-0007 §5; Phase0 never read it).
        if let Some(name) = &styles_part {
            let _ = pkg.xml_part(name);
        }
        for name in theme_part.iter().chain(&numbering_part) {
            let _ = pkg.xml_part(name);
        }
        // Extension metadata is read by Document::comments without a mutable
        // borrow. A malformed extension remains available for lossless saving.
        let main_rels = pkg.relationships(&main)?;
        let comment_extensions: Vec<_> = main_rels.by_type(crate::comments::EX_REL)
            .filter_map(|rel| main_rels.resolve(rel))
            .filter(|part| pkg.contains(part))
            .collect();
        for part in comment_extensions {
            let _ = pkg.xml_part(&part);
        }
        let custom = CustomProperties::read(&pkg)?;

        let mut gates = Vec::new();
        let mut update_fields = None;
        let mut write_protection = None;
        let mut track_revisions = false;
        let mut protected_edit = None;
        let mut comment_protection = None;
        let mut tracked_protection = None;
        let mut control_value_protection = None;
        let mut even_and_odd = false;
        for name in settings {
            let view = SettingsView(pkg.xml_part(&name)?.lens::<Settings>()?);
            // Every occurrence counts (a duplicate is a schema violation, but
            // a later "on" must not hide behind an earlier "off"), and an
            // invalid ST_OnOff value fails closed: the gate is on.
            track_revisions |= view.0.children::<TrackRevisions>().map(TrackRevisionsView).any(|t| t.val().unwrap_or(true));
            let restrictions = view.0.children::<DocumentProtection>().map(DocumentProtectionView).filter_map(|p| {
                let enforced = p.enforcement().unwrap_or(Some(true)).unwrap_or(false);
                let edit = p.edit();
                let restricted = edit.as_ref().is_some_and(|e| e.known() != Some(DocumentProtectionValues::None));
                (enforced && restricted).then(|| edit.map(|e| e.as_str().to_owned()).unwrap_or_default())
            });
            for edit in restrictions {
                if edit != "trackedChanges" && tracked_protection.is_none() {
                    tracked_protection = Some(edit.clone());
                }
                if edit != "comments" && comment_protection.is_none() {
                    comment_protection = Some(edit.clone());
                }
                if edit != "forms" && control_value_protection.is_none() {
                    control_value_protection = Some(edit.clone());
                }
                if protected_edit.is_none() {
                    protected_edit = Some(edit);
                }
            }
            if let Some(u) = view.update_fields_on_open().map(|u| u.val().unwrap_or(true)) {
                update_fields = Some(update_fields.unwrap_or(false) || u);
            }
            even_and_odd |= view.raw().elements().filter(|e| e.is(EVEN_AND_ODD_HEADERS)).any(on_off);
            write_protection = view.raw().elements().filter(|e| e.is(WRITE_PROTECTION)).fold(write_protection, |acc: Option<WriteProtection>, e| {
                let this = WriteProtection {
                    // Advisory: anything but an explicit "off" counts.
                    recommended: e.attr(RECOMMENDED).is_some_and(|v| ooxml_core::value::parse_on_off(v) != Some(false)),
                    has_password: e.attrs().iter().any(|a| matches!(a.name().local(), "hash" | "hashValue")),
                };
                Some(match acc {
                    None => this,
                    Some(a) => WriteProtection { recommended: a.recommended || this.recommended, has_password: a.has_password || this.has_password },
                })
            });
        }
        if track_revisions {
            gates.push(Gate::TrackRevisionsOn);
        }
        if let Some(edit) = protected_edit {
            gates.push(Gate::DocumentProtected(edit));
        }
        let refused = fields::refused(codes, &custom.names, &bookmarks);
        if !refused.is_empty() {
            gates.push(Gate::StaleFieldRisk(refused));
        }
        let main_root = pkg.parsed_part(&main).expect("parsed above").root();
        let uses = section_uses(main_root, &stories[0].index, &rels, even_and_odd);
        for story in &mut stories {
            if let Some(u) = uses.get(&story.id.part) {
                story.sections.clone_from(u);
            }
        }
        let generations = stories.iter().map(|s| vec![0; s.index.paragraphs.len()]).collect();
        Ok(Document {
            pkg,
            main,
            stories,
            generations,
            id: DocumentId::fresh(),
            gen_counter: 0,
            gates,
            update_fields,
            write_protection,
            comment_protection,
            tracked_protection,
            control_value_protection,
            marked_final: custom.marked_final,
            settings_parts,
            styles_part,
            styles_ambiguous,
            annotation_high_water: 0,
            note_high_water: [0; 2],
            theme_part,
            numbering_part,
            fields_opt_in: false,
        })
    }

    /// Serializes the package: every part except an edited one is copied
    /// with its original compressed bytes.
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

    /// This document's identifier, carried by every [`crate::Match`].
    pub fn id(&self) -> DocumentId {
        self.id
    }

    /// `Ok(())` if every write is allowed; otherwise the first
    /// document-level gate, in the order W1 [`Error::TrackRevisionsOn`], W3
    /// [`Error::DocumentProtected`], W2 [`Error::StaleFieldRisk`] (with
    /// every refused field). This is the gate of
    /// [`Operation::Style`], the strictest; [`Self::write_gate_for`] gives
    /// the gate of one operation class. Reads work either way.
    pub fn write_gate(&self) -> Result<()> {
        self.write_gate_for(Operation::Style)
    }

    /// The document-level gates for one operation class (ADR-0007 §6.1):
    /// W1 and W3 as in [`Self::write_gate`], and W2 with only the fields
    /// that operation could make stale.
    ///
    /// After [`Self::enable_update_fields_on_open`], W2 no longer applies to
    /// any operation of this `Document` (ADR-0007 §6.3).
    pub fn write_gate_for(&self, op: Operation) -> Result<()> {
        self.gates
            .iter()
            .filter(|g| !(self.fields_opt_in && matches!(g, Gate::StaleFieldRisk(_))))
            .find_map(|g| g.error_for(op))
            .map_or(Ok(()), Err)
    }

    pub(crate) fn control_gate_for(&self, op: Operation, value: bool) -> Result<()> {
        if !value {
            return self.write_gate_for(op);
        }
        for gate in &self.gates {
            match gate {
                Gate::DocumentProtected(_) => {
                    if let Some(edit) = &self.control_value_protection {
                        return Err(Error::DocumentProtected { edit: edit.clone() });
                    }
                }
                Gate::StaleFieldRisk(_) if self.fields_opt_in => {}
                _ => if let Some(error) = gate.error_for(op) { return Err(error) },
            }
        }
        Ok(())
    }

    pub(crate) fn tracked_protection_gate(&self) -> Result<()> {
        self.tracked_protection.as_ref().map_or(Ok(()), |edit| {
            Err(Error::DocumentProtected { edit: edit.clone() })
        })
    }

    pub(crate) fn tracked_write_gate(&self, op: Operation) -> Result<()> {
        self.tracked_protection_gate()?;
        self.gates.iter()
            .filter(|g| !self.fields_opt_in && matches!(g, Gate::StaleFieldRisk(_)))
            .find_map(|g| g.error_for(op))
            .map_or(Ok(()), Err)
    }

    pub(crate) fn comment_write_gate(&self) -> Result<()> {
        if let Some(edit) = &self.comment_protection {
            return Err(Error::DocumentProtected { edit: edit.clone() });
        }
        Ok(())
    }

    /// W1 and W3 only (the `updateFields` opt-in is a write, not gated by
    /// W2; ADR-0007 §6.3).
    pub(crate) fn write_gate_w1_w3(&self) -> Result<()> {
        self.gates
            .iter()
            .filter(|g| !matches!(g, Gate::StaleFieldRisk(_)))
            .find_map(|g| g.error_for(Operation::Style))
            .map_or(Ok(()), Err)
    }

    /// Every document-level gate that applies to [`Operation::Style`]
    /// (empty if every write is allowed).
    pub fn write_gates(&self) -> Vec<Error> {
        self.gates
            .iter()
            .filter(|g| !(self.fields_opt_in && matches!(g, Gate::StaleFieldRisk(_))))
            .filter_map(|g| g.error_for(Operation::Style))
            .collect()
    }

    /// `settings/w:updateFields` as read (`None` if absent).
    pub fn update_fields_on_open(&self) -> Option<bool> {
        self.update_fields
    }

    /// `settings/w:writeProtection` ("password to modify" or "read-only
    /// recommended"), merged over duplicates; `None` if absent. It guards
    /// saving over the file in Word, not editing its content, so it is
    /// reported and does not gate writes (ADR-0004 §6, "reported").
    pub fn write_protection(&self) -> Option<WriteProtection> {
        self.write_protection
    }

    /// Word's "Mark as Final" (`docProps/custom.xml` property
    /// `_MarkAsFinal` = true). Reported; it does not gate writes (ADR-0004
    /// §6, "reported").
    pub fn is_marked_final(&self) -> bool {
        self.marked_final
    }

    pub(crate) fn story_root_node(&self, story: StoryIndex) -> &RawNode {
        self.pkg
            .parsed_part(&self.stories[story.0].id.part)
            .expect("story parts are parsed at open and never replaced")
            .root()
    }

    /// Every story: the main one first, then headers, footers, footnotes,
    /// endnotes and comments, each in relationship order (ADR-0007 §2.1).
    pub fn stories(&self) -> impl Iterator<Item = Story<'_>> {
        (0..self.stories.len()).map(|i| self.story_at(StoryIndex(i)).expect("in range"))
    }

    /// The story with that identity.
    pub fn story(&self, id: &StoryId) -> Option<Story<'_>> {
        self.stories.iter().position(|s| &s.id == id).and_then(|i| self.story_at(StoryIndex(i)))
    }

    /// The story at that index.
    pub fn story_at(&self, index: StoryIndex) -> Option<Story<'_>> {
        let data = self.stories.get(index.0)?;
        Some(Story { doc: self, index, data, root: self.story_root_node(index) })
    }

    /// The paragraph a [`ParaRef`] names.
    pub fn paragraph_at(&self, p: ParaRef) -> Option<Paragraph<'_>> {
        self.story_at(p.story)?.paragraph(p.index)
    }

    /// The run a [`RunRef`] names.
    pub fn run_at(&self, r: RunRef) -> Option<Run<'_>> {
        self.paragraph_at(r.para)?.run(r.run)
    }

    /// Number of paragraphs in the main story's body flow.
    pub fn paragraph_count(&self) -> usize {
        self.stories[0].index.paragraphs.len()
    }

    /// Paragraphs of the main story's body flow in document order:
    /// `w:body`, descending into tables, content controls and custom XML,
    /// but not into text boxes, drawings or `mc:AlternateContent`.
    pub fn paragraphs(&self) -> impl Iterator<Item = Paragraph<'_>> {
        self.story_at(StoryIndex::MAIN).expect("the main story exists").paragraphs()
    }

    /// Paragraph `index` of the main story (as in [`Self::paragraphs`]).
    pub fn paragraph(&self, index: usize) -> Option<Paragraph<'_>> {
        self.paragraph_at(ParaRef::new(StoryIndex::MAIN, index))
    }

    /// The edit generation of a paragraph (ADR-0007 §7): changed (to a value
    /// never used before) by every write that changes it, and, since
    /// Phase2, by an insert that moves it. Refused calls and writes that leave the
    /// output byte-identical do not change it. 0 for an unknown paragraph.
    pub fn generation(&self, p: ParaRef) -> u64 {
        self.generations.get(p.story.0).and_then(|g| g.get(p.index)).copied().unwrap_or(0)
    }

    /// Re-walks a story after a write that inserted elements on the paths
    /// of its runs (a `w:pPr` before a paragraph's runs, a `w:rPr` before a
    /// run's ruby). Paragraphs and runs keep their indices: nothing is
    /// added to or removed from the flow.
    pub(crate) fn reindex(&mut self, story: StoryIndex) {
        let root = self.story_root_node(story);
        let kind = self.stories[story.0].id.kind;
        let index = match kind {
            StoryKind::Main => body::index(root),
            k => body::index_root(root, story_root(k).1),
        };
        debug_assert_eq!(index.paragraphs.len(), self.stories[story.0].index.paragraphs.len());
        self.stories[story.0].index = index;
    }

    pub(crate) fn bump(&mut self, p: ParaRef) {
        self.gen_counter += 1;
        let next = self.gen_counter;
        if let Some(g) = self.generations.get_mut(p.story.0).and_then(|g| g.get_mut(p.index)) {
            *g = next;
        }
    }

    /// A paragraph was inserted at `at` (Phase2, ADR-0011 §9.1): later
    /// paragraphs move up by one and get fresh generations, so a `Match`
    /// found before the insert is stale for every moved paragraph.
    pub(crate) fn paragraph_inserted(&mut self, at: ParaRef) {
        self.reindex_counted(at.story);
        let g = &mut self.generations[at.story.0];
        g.insert(at.index.min(g.len()), 0);
        for i in at.index..self.generations[at.story.0].len() {
            self.bump(ParaRef::new(at.story, i));
        }
    }

    pub(crate) fn refresh_note_story(&mut self, kind: StoryKind, part: String) {
        let root = self.pkg.parsed_part(&part).expect("committed note part").root();
        let index = body::index_root(root, Mode::Notes);
        if let Some(i) = self.stories.iter().position(|story| story.id.part == part) {
            self.generations[i].resize(index.paragraphs.len(), 0);
            self.stories[i].index = index;
        } else {
            self.generations.push(vec![0; index.paragraphs.len()]);
            self.stories.push(StoryData { id: StoryId { kind, part }, index, sections: Vec::new() });
        }
    }

    /// Re-walks a story whose paragraph count changed.
    pub(crate) fn reindex_counted(&mut self, story: StoryIndex) {
        let root = self.story_root_node(story);
        let kind = self.stories[story.0].id.kind;
        self.stories[story.0].index = match kind {
            StoryKind::Main => body::index(root),
            k => body::index_root(root, story_root(k).1),
        };
    }

    /// Replaces the content of run `run` of paragraph `paragraph` of the
    /// main story with `text`. See [`Self::set_run_text_at`].
    pub fn set_run_text(&mut self, paragraph: usize, run: usize, text: &str) -> Result<()> {
        if self.paragraph(paragraph).is_none() {
            return Err(Error::NoSuchParagraph(paragraph));
        }
        self.set_run_text_at(RunRef::new(ParaRef::new(StoryIndex::MAIN, paragraph), run), text)
    }

    /// Replaces the content of a run of any story with `text` (ADR-0004
    /// §3): `\t` → `w:tab`, `\n`/`\r\n`/`\r` → `w:br`, the rest → `w:t`
    /// (with `xml:space="preserve"` where needed). `w:rPr` and all
    /// attributes are kept; a `w:lastRenderedPageBreak` in the run is
    /// dropped. Writing content the run already has is a no-op, unless a
    /// `w:t` of the run needs `xml:space="preserve"` (edge or double spaces)
    /// and does not carry it: Word would not display the requested text, so
    /// the run is rewritten (ADR-0004 §3.2 step 6).
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Text`]), [`Run::check_settable`],
    /// [`Error::InvalidXmlChar`], [`Error::NoSuchParagraph`],
    /// [`Error::NoSuchRun`].
    pub fn set_run_text_at(&mut self, run: RunRef, text: &str) -> Result<()> {
        self.write_gate_for(Operation::Text)?;
        let (paragraph, index) = (run.para.index, run.run);
        let r = self.paragraph_at(run.para).ok_or(Error::NoSuchParagraph(paragraph))?;
        let r = r.run(index).ok_or(Error::NoSuchRun { paragraph, run: index })?;
        r.check_settable()?;
        let pieces = map_text(text)?;
        if current_pieces(r.node).as_ref() == Some(&pieces) && !lacks_needed_preserve(r.node) {
            return Ok(());
        }
        self.with_run(run, |lens| write_pieces(lens, &pieces))?;
        self.bump(run.para);
        Ok(())
    }

    /// Runs `f` on a mutable lens on the run `at`. Only paths through the
    /// story root, a note or comment, `w:body`, `w:tbl`/`w:tr`/`w:tc`, `w:p`
    /// and `w:hyperlink` are followed; those are the only ones a settable
    /// run has ([`Error::NoSuchRun`] otherwise).
    pub(crate) fn with_run<T>(&mut self, at: RunRef, f: impl FnOnce(&mut LensMut<'_, R>) -> Result<T>) -> Result<T> {
        let missing = || Error::NoSuchRun { paragraph: at.para.index, run: at.run };
        let data = self.stories.get(at.para.story.0).ok_or_else(missing)?;
        let info = data.index.paragraphs.get(at.para.index).and_then(|p| p.runs.get(at.run)).ok_or_else(missing)?;
        let path = data.index.path(info.frame);
        let kind = data.id.kind;
        let xml = self.pkg.xml_part_mut(&data.id.part)?;
        let (p, rest) = story_para(xml, kind, &path)?.ok_or_else(missing)?;
        f(&mut run_below(p, rest).ok_or_else(missing)?)
    }

    /// Runs `f` on a mutable lens on paragraph `at`, reached as
    /// [`Self::with_run`] reaches runs ([`Error::NoSuchParagraph`] for a
    /// paragraph inside anything else, e.g. a block-level content control).
    pub(crate) fn with_para<T>(&mut self, at: ParaRef, f: impl FnOnce(&mut LensMut<'_, P>) -> Result<T>) -> Result<T> {
        let missing = || Error::NoSuchParagraph(at.index);
        let data = self.stories.get(at.story.0).ok_or_else(missing)?;
        let info = data.index.paragraphs.get(at.index).ok_or_else(missing)?;
        let path = data.index.path(info.frame);
        let kind = data.id.kind;
        let xml = self.pkg.xml_part_mut(&data.id.part)?;
        match story_para(xml, kind, &path)? {
            Some((mut p, [])) => f(&mut p),
            _ => Err(missing()),
        }
    }

    /// Appends a paragraph with `text` as the last block of the main
    /// story's body, before the body-level `w:sectPr` (or at the end of the
    /// body if there is none). It has no `w:pPr`, `w:rPr` or IDs; empty
    /// `text` gives an empty paragraph. Returns the new paragraph's index.
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Append`]), [`Error::InvalidXmlChar`],
    /// [`Error::NoBody`], [`Error::InsideField`] (the body ends inside an
    /// unterminated field), A1 `Core(UnresolvedSibling)` (the body has an
    /// `mc:AlternateContent` child), A2 [`Error::SectionBreakAtEnd`] only
    /// when the body ends with a paragraph carrying `w:pPr/w:sectPr` inside a
    /// table or content control. Since Phase2 (ADR-0011 §6), when that
    /// paragraph is a body child, its `w:sectPr` moves to the new paragraph
    /// (copied with its header/footer references written again, then
    /// removed), so the new paragraph stays in the last section; the move's
    /// refusals are those of [`Self::insert_section_break`].
    pub fn append_paragraph(&mut self, text: &str) -> Result<usize> {
        self.write_gate_for(Operation::Append)?;
        let pieces = map_text(text)?;
        let index = &self.stories[0].index;
        let b = index.body.ok_or(Error::NoBody)?;
        if index.open_fields_at_end > 0 {
            return Err(Error::InsideField);
        }
        if ends_with_section_break(self.story_root_node(StoryIndex::MAIN), index) {
            // Phase2 lifts A2 (ADR-0011 §6): the last section's sectPr moves to
            // the new paragraph, when the last paragraph is a body child.
            return self.append_moving_section(&pieces, &crate::ParagraphSpec::Plain, None);
        }
        let xml = self.pkg.xml_part_mut(&self.main)?;
        let mut body = xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(b).ok_or(Error::NoBody)?;
        let mut p = body.insert::<P>()?;
        if !pieces.is_empty() {
            write_pieces(&mut p.insert::<R>()?, &pieces)?;
        }
        let root = self.story_root_node(StoryIndex::MAIN);
        self.stories[0].index = body::index(root);
        let n = self.stories[0].index.paragraphs.len();
        self.generations[0].resize(n, 0);
        Ok(n - 1)
    }
}

fn on_off(e: &RawNode) -> bool {
    e.attr(VAL).is_none_or(|v| ooxml_core::value::parse_on_off(v) != Some(false))
}

/// ADR-0007 §2.3: for each header or footer part, the sections that use it.
fn section_uses(
    root: &RawNode,
    index: &BodyIndex,
    rels: &ooxml_core::opc::Relationships,
    even_and_odd: bool,
) -> std::collections::HashMap<String, Vec<SectionUse>> {
    let mut sect_prs: Vec<&RawNode> = index
        .paragraphs
        .iter()
        .flat_map(|p| node_at(root, &index.path(p.frame)).elements().filter(|e| is::<ParagraphProperties>(e)))
        .flat_map(|ppr| ppr.elements().filter(|e| is::<SectionProperties>(e)))
        .collect();
    if let Some(body) = index.body.and_then(|b| root.children()[b].as_element()) {
        sect_prs.extend(body.elements().filter(|e| is::<SectionProperties>(e)));
    }
    const TYPES: [HeaderFooterType; 3] = [HeaderFooterType::Default, HeaderFooterType::First, HeaderFooterType::Even];
    // [header, footer] × [default, first, even]
    let mut current: [[Option<String>; 3]; 2] = Default::default();
    let mut out: std::collections::HashMap<String, Vec<SectionUse>> = std::collections::HashMap::new();
    for (section, sp) in sect_prs.iter().enumerate() {
        for (hf, q) in [HEADER_REFERENCE, FOOTER_REFERENCE].into_iter().enumerate() {
            for r in sp.elements().filter(|e| e.is(q)) {
                let t = match r.attr(W_TYPE) {
                    None | Some("default") => 0,
                    Some("first") => 1,
                    Some("even") => 2,
                    Some(_) => continue,
                };
                let part = r
                    .attr(R_ID)
                    .and_then(|id| rels.by_id(id))
                    .filter(|rel| rel.target_mode == TargetMode::Internal)
                    .and_then(|rel| rels.resolve(rel));
                current[hf][t] = part;
            }
        }
        let title_pg = sp.elements().filter(|e| e.is(TITLE_PG)).any(on_off);
        for hf in &current {
            for (t, part) in hf.iter().enumerate() {
                if let Some(part) = part {
                    let kind = TYPES[t];
                    let displayed = match kind {
                        HeaderFooterType::First => title_pg,
                        HeaderFooterType::Even => even_and_odd,
                        HeaderFooterType::Default => true,
                    };
                    out.entry(part.clone()).or_default().push(SectionUse { section, kind, displayed });
                }
            }
        }
    }
    out
}

/// A2: the body has no `w:sectPr` of its own and the last paragraph of the
/// body flow (in document order, at any depth) has a `w:pPr/w:sectPr`. That
/// paragraph ends the last section, and no paragraph follows it, so a
/// paragraph appended at the end of the body would start a new section
/// with default page settings (review round 2 m2; round 3 m8: blocks
/// without paragraphs after it, such as an empty `w:sdt` or a table without
/// rows, do not change this).
pub(crate) fn ends_with_section_break(root: &RawNode, index: &BodyIndex) -> bool {
    let Some(body) = index.body.and_then(|b| root.children()[b].as_element()) else { return false };
    if body.elements().any(is::<SectionProperties>) {
        return false;
    }
    let Some(para) = index.paragraphs.last() else { return false };
    node_at(root, &index.path(para.frame))
        .elements()
        .filter(|e| is::<ParagraphProperties>(e))
        .any(|ppr| ppr.elements().any(is::<SectionProperties>))
}

/// One step towards a paragraph: the paragraph itself, or the cell of the
/// table at the head of the path.
enum Step<'a, 'p> {
    Para(LensMut<'a, P>, &'p [usize]),
    Cell(LensMut<'a, TableCell>, &'p [usize]),
}

fn step<'a, 'p, E>(c: LensMut<'a, E>, path: &'p [usize]) -> Option<Step<'a, 'p>>
where
    E: ElementSpec,
    P: ChildOf<E>,
    Table: ChildOf<E>,
{
    let (&i, rest) = path.split_first()?;
    if c.node().children().get(i)?.as_element().is_some_and(is::<P>) {
        return Some(Step::Para(c.into_child_at::<P>(i)?, rest));
    }
    let [r, k, rest @ ..] = rest else { return None };
    let tc = c.into_child_at::<Table>(i)?.into_child_at::<TableRow>(*r)?.into_child_at::<TableCell>(*k)?;
    Some(Step::Cell(tc, rest))
}

/// The paragraph on `path` (from the story part's root) and the rest of
/// the path below it. `Ok(None)` when the path does not lead to a paragraph
/// through the story root, a note or comment, `w:body` and tables.
fn story_para<'a, 'p>(xml: &'a mut XmlDocument, kind: StoryKind, path: &'p [usize]) -> Result<Option<(LensMut<'a, P>, &'p [usize])>> {
    let Some((&first, rest)) = path.split_first() else { return Ok(None) };
    Ok(match kind {
        StoryKind::Main => xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(first).and_then(|b| para_in(b, rest)),
        StoryKind::Header => para_in(xml.lens_mut::<Header>()?, path),
        StoryKind::Footer => para_in(xml.lens_mut::<Footer>()?, path),
        StoryKind::Footnotes => xml.lens_mut::<Footnotes>()?.into_child_at::<Footnote>(first).and_then(|n| para_in(n, rest)),
        StoryKind::Endnotes => xml.lens_mut::<Endnotes>()?.into_child_at::<Endnote>(first).and_then(|n| para_in(n, rest)),
        StoryKind::Comments => xml.lens_mut::<Comments>()?.into_child_at::<Comment>(first).and_then(|n| para_in(n, rest)),
    })
}

/// The paragraph at the head of `path` below `container` (a body, header,
/// footer, note or comment, through tables), and the rest of the path.
/// Iterative, like the walks.
fn para_in<'a, 'p, E>(container: LensMut<'a, E>, path: &'p [usize]) -> Option<(LensMut<'a, P>, &'p [usize])>
where
    E: ElementSpec,
    P: ChildOf<E>,
    Table: ChildOf<E>,
{
    let mut at = step(container, path)?;
    loop {
        match at {
            Step::Para(p, rest) => return Some((p, rest)),
            Step::Cell(tc, rest) => at = step::<TableCell>(tc, rest)?,
        }
    }
}

/// The run at `rest` below paragraph `p`, through `w:hyperlink`s only.
fn run_below<'a>(p: LensMut<'a, P>, rest: &[usize]) -> Option<LensMut<'a, R>> {
    let (&i, mut rest) = rest.split_first()?;
    if rest.is_empty() {
        return p.into_child_at::<R>(i);
    }
    let mut link = p.into_child_at::<Hyperlink>(i)?;
    loop {
        let (&i, tail) = rest.split_first()?;
        if tail.is_empty() {
            return link.into_child_at::<R>(i);
        }
        link = link.into_child_at::<Hyperlink>(i)?;
        rest = tail;
    }
}

/// Replaces the run's content (everything but `w:rPr` and whitespace-only
/// text) with `pieces`. Fast path: one `w:t` replaced by one `w:t` keeps
/// the element and its open tag.
pub(crate) fn write_pieces(run: &mut LensMut<'_, R>, pieces: &[Piece]) -> Result<()> {
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

pub(crate) fn set_t(t: &mut LensMut<'_, Text>, s: &str) -> Result<()> {
    if needs_preserve(s) {
        t.set_attr(XML_SPACE, "preserve")?;
    }
    t.set_text(s)?;
    Ok(())
}

pub(crate) fn node_at<'a>(root: &'a RawNode, path: &[usize]) -> &'a RawNode {
    path.iter().fold(root, |n, &i| n.children()[i].as_element().expect("indexed paths point at elements"))
}

/// A paragraph of a story's flow.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'d> {
    pub(crate) doc: &'d Document,
    story: StoryIndex,
    pub(crate) data: &'d StoryData,
    root: &'d RawNode,
    pub(crate) info: &'d ParaInfo,
    index: usize,
}

impl<'d> Paragraph<'d> {
    pub(crate) fn new(doc: &'d Document, story: StoryIndex, data: &'d StoryData, root: &'d RawNode, index: usize) -> Self {
        Paragraph { doc, story, data, root, info: &data.index.paragraphs[index], index }
    }

    /// Index in its story's paragraphs ([`Document::paragraphs`] for the
    /// main story).
    pub fn index(&self) -> usize {
        self.index
    }

    /// This paragraph as a [`ParaRef`].
    pub fn para_ref(&self) -> ParaRef {
        ParaRef::new(self.story, self.index)
    }

    /// The paragraph's edit generation ([`Document::generation`]).
    pub fn generation(&self) -> u64 {
        self.doc.generation(self.para_ref())
    }

    /// What holds the paragraph.
    pub fn container(&self) -> Container {
        let note = self.info.note.map(|n| &self.data.index.notes[n as usize]);
        match (self.data.id.kind, note) {
            (k @ (StoryKind::Footnotes | StoryKind::Endnotes), Some(n)) => {
                Container::Note { kind: k, id: n.id.clone(), separator: n.separator }
            }
            (StoryKind::Comments, Some(n)) => Container::Comment { id: n.id.clone() },
            _ => Container::Body,
        }
    }

    pub(crate) fn in_separator_note(&self) -> bool {
        self.info.note.is_some_and(|n| self.data.index.notes[n as usize].separator)
    }

    /// The `w:p` element.
    pub fn raw(&self) -> &'d RawNode {
        node_at(self.root, &self.data.index.path(self.info.frame))
    }

    pub(crate) fn table_context(&self) -> Option<(&'d RawNode, &'d RawNode, &'d RawNode)> {
        let mut node = self.root;
        let (mut table, mut row, mut cell) = (None, None, None);
        for index in self.data.index.path(self.info.frame) {
            node = node.indexed_elements().find(|(i, _)| *i == index)?.1;
            if node.is(Table::QNAME) {
                (table, row, cell) = (Some(node), None, None);
            } else if node.is(TableRow::QNAME) {
                (row, cell) = (Some(node), None);
            } else if node.is(TableCell::QNAME) {
                cell = Some(node);
            }
        }
        Some((table?, row?, cell?))
    }

    /// The "all revisions accepted" text: the text of [`Self::runs`] except
    /// runs inside `w:del` or `w:moveFrom`. Field results are included,
    /// field instructions are not (they are `w:instrText`).
    pub fn text(&self) -> String {
        self.runs().filter(|r| !r.is_removed()).map(|r| r.text()).collect()
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
        let this = *self;
        (0..self.info.runs.len()).map(move |i| this.run(i).expect("in range"))
    }

    /// Run `index` (as in [`Self::runs`]).
    pub fn run(&self, index: usize) -> Option<Run<'d>> {
        let info = self.info.runs.get(index)?;
        Some(Run {
            paragraph: *self,
            node: node_at(self.root, &self.data.index.path(info.frame)),
            info,
            index,
        })
    }
}

/// A run (`w:r`) in a paragraph's flow.
#[derive(Clone, Copy, Debug)]
pub struct Run<'d> {
    pub(crate) paragraph: Paragraph<'d>,
    pub(crate) node: &'d RawNode,
    pub(crate) info: &'d RunInfo,
    index: usize,
}

impl<'d> Run<'d> {
    pub(crate) fn wrappers(&self) -> Vec<Wrapper> {
        self.paragraph.data.index.wrappers(self.info.frame)
    }

    /// Inside `w:del` or `w:moveFrom` (at any depth): the run's text is not
    /// part of [`Paragraph::text`].
    pub fn is_removed(&self) -> bool {
        self.wrappers().iter().any(|w| w.is_removed_text())
    }

    /// Index in [`Paragraph::runs`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// This run as a [`RunRef`].
    pub fn run_ref(&self) -> RunRef {
        RunRef::new(self.paragraph.para_ref(), self.index)
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
    /// between the story's container and the run, else
    /// [`RunContext::Hyperlink`] if it is in a hyperlink, else
    /// [`RunContext::Direct`].
    pub fn context(&self) -> RunContext {
        let ws = &self.wrappers();
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

    /// Does the run's `w:rPr` record a tracked formatting change
    /// (`w:rPrChange`)? Such a run is not a text run for
    /// [`Document::replace`] (ADR-0007 §1.1).
    pub fn has_formatting_change(&self) -> bool {
        self.node.elements().filter(|e| is::<wml::RunProperties>(e)).any(|rpr| rpr.elements().any(|e| e.is(RPR_CHANGE)))
    }

    /// Why [`Document::set_run_text_at`] would refuse this run, without
    /// trying (the document-level gates are not included). Checked in
    /// order: R1 [`Error::InsideField`], R2 [`Error::InsideRevision`], R3
    /// [`Error::UnsupportedContext`] (including the runs of separator notes,
    /// ADR-0007 §2.2), R4 [`Error::RunNotTextOnly`].
    pub fn check_settable(&self) -> Result<()> {
        let ws = &self.wrappers();
        if self.info.in_field || ws.contains(&Wrapper::SimpleField) {
            return Err(Error::InsideField);
        }
        crate::revisions::check_run_revision(self)?;
        if self.paragraph.in_separator_note() {
            let ancestor = if self.paragraph.data.id.kind == StoryKind::Endnotes { "w:endnote (separator)" } else { "w:footnote (separator)" };
            return Err(Error::UnsupportedContext { ancestor });
        }
        if let Some(w) = ws.iter().find(|w| **w != Wrapper::Hyperlink) {
            return Err(Error::UnsupportedContext { ancestor: w.qname() });
        }
        check_text_only(self.node)
    }
}

/// What `docProps/custom.xml` tells the gates.
#[derive(Default)]
struct CustomProperties {
    /// Normalised custom property names (for W2's `DOCPROPERTY`).
    names: fields::CustomNames,
    /// `_MarkAsFinal` is true.
    marked_final: bool,
}

impl CustomProperties {
    /// Reads every part named by a package-level custom-properties
    /// relationship, transiently (they stay raw copies on save).
    fn read(pkg: &Package) -> Result<Self> {
        let rels = pkg.package_relationships();
        let mut out = Self::default();
        for name in rels.by_type(CUSTOM_PROPERTIES_RELATIONSHIP).filter_map(|r| rels.resolve(r)).filter(|n| pkg.contains(n)) {
            let doc = XmlDocument::parse(&pkg.read_part(&name)?)?;
            let attr = |e: &RawNode, local| e.attr(QName::new("", local)).map(str::to_owned);
            for prop in doc.root().elements().filter(|e| e.is(QName::new(CUSTOM_PROPERTIES_NS, "property"))) {
                let name = attr(prop, "name").unwrap_or_default();
                if attr(prop, "linkTarget").is_some() {
                    out.names.linked.push(fields::normalize_name(&name));
                }
                out.names.all.push(fields::normalize_name(&name));
                if name == "_MarkAsFinal" {
                    let value = prop.elements().next().map(|v| v.text().into_owned()).unwrap_or_default();
                    out.marked_final |= ooxml_core::value::parse_on_off(&value) == Some(true);
                }
            }
        }
        Ok(out)
    }
}

impl Document {
    /// Parts to scan for registry references. Content types also find glossary
    /// and auxiliary parts that are outside the public story index.
    pub(crate) fn registry_parts(&self, additional: &[&str]) -> Result<Vec<String>> {
        self.registry_parts_with_glossary(additional, true)
    }

    pub(crate) fn registry_parts_with_glossary(&self, additional: &[&str], glossary: bool) -> Result<Vec<String>> {
        const TYPE_PREFIX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.";
        const REL_PREFIX: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
        const EFFECTS_TYPE: &str = "application/vnd.ms-word.stylesWithEffects+xml";
        const EFFECTS_REL: &str = "http://schemas.microsoft.com/office/2007/relationships/stylesWithEffects";
        let include_effects = additional.contains(&"stylesWithEffects");
        let story_kind = |kind| matches!(kind, "header" | "footer" | "endnotes" | "footnotes" | "comments");
        let mut parts: std::collections::BTreeSet<_> = self.stories.iter().map(|s| s.id.part.clone()).collect();
        parts.extend(self.pkg.part_names().filter(|name| {
            let content_type = self.pkg.content_type(name);
            (include_effects && content_type == Some(EFFECTS_TYPE))
                || content_type.and_then(|t| t.strip_prefix(TYPE_PREFIX))
                .and_then(|t| t.strip_suffix("+xml"))
                .is_some_and(|kind| story_kind(kind) || (glossary && kind == "document.glossary") || additional.contains(&kind))
        }).map(str::to_owned));
        let rels = self.pkg.relationships(self.main_part_name())?;
        parts.extend(rels.iter().filter(|r| (include_effects && r.rel_type == EFFECTS_REL) || r.rel_type.strip_prefix(REL_PREFIX)
            .is_some_and(|kind| story_kind(kind) || (glossary && kind == "glossaryDocument") || additional.contains(&kind)))
            .filter_map(|r| rels.resolve(r)).filter(|name| self.pkg.contains(name)));
        Ok(parts.into_iter().collect())
    }
}
