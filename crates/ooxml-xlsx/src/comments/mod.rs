//! Worksheet notes and threaded comments (ADR-0017 §§4–5).

mod notes;
mod threads;
mod vml;
mod xml;

pub use notes::{Note, NoteBox, NoteEditAs};
pub use threads::{Comment, Person, PersonId};

use ooxml_core::{ElementSpec, NsDecl, PartNameRequest, RegistryPlan, RegistryRefusal, XmlDocument};
use ooxml_schema::sml;

use crate::images::{REL_VML, part_error, record_shape};
use crate::workbook::ApplyStep;
use crate::{CellIndex, Error, Result, Worksheet};

const REL_NOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const NOTES_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";
const REL_NS: [NsDecl<'static>; 1] = [NsDecl::new("r", xml::RID.ns)];

enum Operation<'a> {
    Add(&'a [Note]),
    ReplaceLegacy(&'a [Note]),
    SetText(&'a str),
    SetRuns(&'a [crate::RichTextRun], Option<&'a NoteBox>),
    RemoveNote,
    AddThread(Comment),
    Reply(Comment),
    RemoveThread,
}

/// Rich text runs with fonts as written (see [`crate::DeclaredFont`]).
type DeclaredRuns = Vec<(String, Option<crate::DeclaredFont>)>;

/// A legacy note as stored, for readers that report its markup (SheetJS `r`
/// and `h`).
#[derive(Clone, Debug, PartialEq)]
pub struct NoteSource {
    /// The cell the note belongs to.
    pub at: CellIndex,
    /// The author name (empty without a valid `authorId`).
    pub author: String,
    /// The note text, as [`Worksheet::note_text`] reads it.
    pub text: String,
    /// The XML inside the note's `text` element as stored; `None` without one.
    pub text_xml: Option<String>,
    /// The runs with fonts as written when the text is made of `r` runs;
    /// `None` for plain text or when a run font cannot be read.
    pub runs: Option<DeclaredRuns>,
}

impl Worksheet<'_> {
    /// Reads the text of a legacy note, excluding threaded comment fallbacks.
    pub fn note_text(&self, at: CellIndex) -> Result<Option<String>> {
        self.with_legacy_note(at, notes::text)
    }

    /// Reads the text runs and supported fonts of a legacy note.
    pub fn note_runs(&self, at: CellIndex) -> Result<Option<Vec<crate::RichTextRun>>> {
        self.with_legacy_note(at, |note| {
            let runs = note.elements().find(|child| child.is(sml::CommentText::QNAME))
                .and_then(|text| crate::registry::read::rich_text_runs(text).ok())
                .unwrap_or_default();
            if runs.is_empty() {
                vec![(notes::text(note), None)]
            } else {
                runs
            }
        })
    }

    /// Like [`Self::note_runs`], but each run font is read as written: name
    /// and size may be absent, and `outline` / `shadow` are accepted
    /// (see [`crate::DeclaredFont`]).
    pub fn declared_note_runs(&self, at: CellIndex) -> Result<Option<DeclaredRuns>> {
        self.with_legacy_note(at, |note| {
            let runs = note.elements().find(|child| child.is(sml::CommentText::QNAME))
                .and_then(|text| crate::registry::read::declared_rich_text_runs(text).ok())
                .unwrap_or_default();
            if runs.is_empty() {
                vec![(notes::text(note), None)]
            } else {
                runs
            }
        })
    }

    /// Reads the explicitly stored box options of a legacy note.
    pub fn note_box_fields(&mut self, at: CellIndex) -> Result<Vec<String>> {
        CellIndex::new(at.row, at.col)?;
        let sheet = self.part_name().to_owned();
        let Some((part, doc)) = vml::read(&mut self.wb.pkg, &sheet)? else { return Ok(Vec::new()) };
        Ok(vml::note_shape(&part, &doc, at)?.map(vml::note_box_fields).unwrap_or_default())
    }

    fn with_legacy_note<T>(&self, at: CellIndex, read: impl FnOnce(&ooxml_core::RawNode) -> T) -> Result<Option<T>> {
        CellIndex::new(at.row, at.col)?;
        let sheet = self.part_name();
        let pkg = &self.wb.pkg;
        let Some(part) = xml::target(pkg, sheet, REL_NOTES)? else { return Ok(None) };
        if let Some((part, doc)) = threads::read(pkg, sheet)?
            && !threads::at(&part, &doc, at)?.is_empty()
        {
            return Ok(None);
        }
        let doc = xml::read(pkg, &part, notes::ROOT)?;
        let notes = notes::Notes::new(&doc, &part)?;
        Ok(notes.at(at)?.map(read))
    }

    /// Lists legacy note coordinates without decoding authors or text.
    pub fn note_positions(&self) -> Result<Vec<CellIndex>> {
        self.collect_legacy_notes(|notes| notes.positions().map(|at| (at, at)).collect())
    }

    /// Lists legacy note coordinates, authors and text in document order.
    pub fn cell_notes(&self) -> Result<Vec<(CellIndex, String, String)>> {
        self.collect_legacy_notes(|notes| notes.entries(None).map(|entry| (entry.0, entry)).collect())
    }

    /// Lists legacy notes in document order with their stored `text` markup
    /// and declared run fonts (see [`NoteSource`]).
    pub fn note_sources(&self) -> Result<Vec<NoteSource>> {
        self.collect_legacy_notes(|notes| notes.sources().collect())
    }

    /// The visibility of each VML note shape, in document order: `true` when
    /// its `x:ClientData` has `x:Visible`. Empty without a legacy drawing.
    pub fn note_visibility(&mut self) -> Result<Vec<(CellIndex, bool)>> {
        let sheet = self.part_name().to_owned();
        let Some((_, doc)) = vml::read(&mut self.wb.pkg, &sheet)? else { return Ok(Vec::new()) };
        Ok(vml::note_visibility(&doc))
    }

    /// Lists legacy notes on one zero-based row in document order.
    pub fn row_notes(&self, row: u32) -> Result<Vec<(CellIndex, String, String)>> {
        self.collect_legacy_notes(|notes| notes.entries(Some(row)).map(|entry| (entry.0, entry)).collect())
    }

    /// Lists legacy note coordinates on one zero-based row.
    pub fn row_note_positions(&self, row: u32) -> Result<Vec<CellIndex>> {
        self.collect_legacy_notes(|notes| notes.positions().filter(|at| at.row == row).map(|at| (at, at)).collect())
    }

    fn collect_legacy_notes<T>(&self, entries: impl FnOnce(&notes::Notes<'_>) -> Vec<(CellIndex, T)>) -> Result<Vec<T>> {
        let sheet = self.part_name();
        let pkg = &self.wb.pkg;
        let Some(part) = xml::target(pkg, sheet, REL_NOTES)? else { return Ok(Vec::new()) };
        let doc = xml::read(pkg, &part, notes::ROOT)?;
        let notes = notes::Notes::new(&doc, &part)?;
        let thread = threads::read(pkg, sheet)?;
        let entries = entries(&notes);
        let Some((part, doc)) = thread else {
            return Ok(entries.into_iter().map(|(_, value)| value).collect());
        };
        let mut legacy = Vec::with_capacity(entries.len());
        for (at, value) in entries {
            if threads::at(&part, &doc, at)?.is_empty() {
                legacy.push(value);
            }
        }
        Ok(legacy)
    }

    /// Adds a legacy note to a cell that has no note or thread.
    pub fn add_note(&mut self, at: CellIndex, note: Note) -> Result<()> {
        self.add_legacy_notes(at, std::slice::from_ref(&note))
    }

    /// Adds legacy notes at the same cell in order, with a shared VML shape.
    /// The first note controls the shape. Refuses cells with an existing note or thread.
    pub fn add_legacy_notes(&mut self, at: CellIndex, notes: &[Note]) -> Result<()> {
        let Some(first) = notes.first() else {
            return Ok(());
        };
        if let Some(settings) = &first.box_settings {
            settings.validate()?;
        }
        self.edit_comments(at, Operation::Add(notes))
    }

    /// Replaces all legacy comments at a cell, retaining its existing VML shape.
    pub fn replace_legacy_notes(&mut self, at: CellIndex, notes: &[Note]) -> Result<()> {
        if let Some(settings) = notes.first().and_then(|note| note.box_settings.as_ref()) {
            settings.validate()?;
        }
        self.edit_comments(at, Operation::ReplaceLegacy(notes))
    }

    /// Replaces a legacy note's text while preserving its author and VML shape.
    pub fn set_note_text(&mut self, at: CellIndex, text: &str) -> Result<()> {
        self.edit_comments(at, Operation::SetText(text))
    }

    /// Replaces the rich text while preserving the note author and VML shape.
    pub fn set_note_runs(&mut self, at: CellIndex, runs: &[crate::RichTextRun]) -> Result<()> {
        self.edit_comments(at, Operation::SetRuns(runs, None))
    }

    /// Replaces the note text and explicit VML box options.
    pub fn set_note_model(&mut self, at: CellIndex, runs: &[crate::RichTextRun], settings: &NoteBox) -> Result<()> {
        settings.validate()?;
        self.edit_comments(at, Operation::SetRuns(runs, Some(settings)))
    }

    /// Removes a legacy note and its matching VML shape.
    pub fn remove_note(&mut self, at: CellIndex) -> Result<()> {
        self.edit_comments(at, Operation::RemoveNote)
    }

    /// Creates a threaded comment and the legacy note that represents it.
    pub fn add_comment(&mut self, at: CellIndex, comment: Comment) -> Result<()> {
        self.edit_comments(at, Operation::AddThread(comment))
    }

    /// Appends a reply to the thread on this cell and updates its fallback note.
    pub fn reply(&mut self, at: CellIndex, comment: Comment) -> Result<()> {
        self.edit_comments(at, Operation::Reply(comment))
    }

    /// Removes the root and replies together with their fallback note and shape.
    pub fn remove_comment_thread(&mut self, at: CellIndex) -> Result<()> {
        self.edit_comments(at, Operation::RemoveThread)
    }

    fn edit_comments(&mut self, at: CellIndex, operation: Operation<'_>) -> Result<()> {
        CellIndex::new(at.row, at.col)?;
        let sheet = self.part_name().to_owned();
        let wb = &mut *self.wb;
        wb.check_editable()?;
        let thread = threads::read(&wb.pkg, &sheet)?;
        let entries = thread
            .as_ref()
            .map(|(part, doc)| threads::at(part, doc, at))
            .transpose()?
            .unwrap_or_default();
        let thread_root = thread.as_ref().map(|(_, doc)| doc.root());
        let is_thread = !entries.is_empty();
        if is_thread
            && matches!(
                operation,
                Operation::ReplaceLegacy(_) | Operation::SetText(_) | Operation::SetRuns(..) | Operation::RemoveNote | Operation::AddThread(_)
            )
        {
            return Err(Error::ThreadedComment);
        }
        if !is_thread && matches!(operation, Operation::Reply(_) | Operation::RemoveThread) {
            return Err(Error::NoSuchNote);
        }
        let legacy = vml::read(&mut wb.pkg, &sheet)?;
        let notes_part = xml::target(&wb.pkg, &sheet, REL_NOTES)?;
        let doc = if let Some(part) = &notes_part {
            xml::read(&wb.pkg, part, notes::ROOT)?
        } else {
            XmlDocument::parse(notes::EMPTY.as_bytes())?
        };
        let notes = notes::Notes::new(&doc, notes_part.as_deref().unwrap_or(&sheet))?;
        let current = if matches!(operation, Operation::ReplaceLegacy(_)) {
            None
        } else {
            notes.at(at)?
        };
        let mut added = None;
        let mut updated_box = None;
        let mut remove_note_shape = false;
        let mut thread_markup = None;
        let mut new_guid = None;
        let mut remove_thread = false;
        let note_edits = match operation {
            Operation::Add(new_notes) => {
                if is_thread && current.is_none() {
                    return Err(Error::ThreadedComment);
                }
                let edits = notes.append_all(at, new_notes, None)?;
                let note = &new_notes[0];
                added = Some(vml::append(wb, &sheet, legacy.as_ref(), at, note.visible, note.box_settings.as_ref())?);
                edits
            }
            Operation::ReplaceLegacy(new_notes) => {
                let (edits, had_notes) = notes.replace_all(at, new_notes)?;
                match (had_notes, new_notes.first()) {
                    (false, None) => return Ok(()),
                    (false, Some(note)) => {
                        added = Some(vml::append(wb, &sheet, legacy.as_ref(), at, note.visible, note.box_settings.as_ref())?);
                    }
                    (true, None) => remove_note_shape = true,
                    (true, Some(_)) => {}
                }
                edits
            }
            Operation::SetText(text) => {
                let current = current.ok_or(Error::NoSuchNote)?;
                vec![notes.set_text(current, text)?]
            }
            Operation::SetRuns(runs, settings) => {
                let current = current.ok_or(Error::NoSuchNote)?;
                if let Some(settings) = settings {
                    let (part, doc) = legacy.as_ref().ok_or_else(|| part_error(&sheet, "missing VML note drawing"))?;
                    updated_box = Some((part, doc, vml::update_note_box(part, doc, at, settings)?));
                }
                vec![notes.set_runs(current, runs)?]
            }
            Operation::RemoveNote => {
                let current = current.ok_or(Error::NoSuchNote)?;
                remove_note_shape = true;
                vec![xml::replace(current, String::new())]
            }
            Operation::AddThread(comment) => {
                if current.is_some() {
                    return Err(Error::NoteExists);
                }
                let id = threads::guid(wb)?;
                let markup = comment.markup(wb, at, &id, None, thread_root)?;
                let fallback =
                    Note::new(format!("{}{text}", threads::FALLBACK, text = comment.text))
                        .author(format!("tc={id}"));
                let edits = notes.append(at, &fallback, Some(&id))?;
                added = Some(vml::append(wb, &sheet, legacy.as_ref(), at, false, None)?);
                thread_markup = Some(markup);
                new_guid = Some(id);
                edits
            }
            Operation::Reply(_) | Operation::RemoveThread => {
                let (part, _) = thread.as_ref().expect("entries have a part");
                let id = threads::root_id(part, &entries)?;
                let current = current
                    .filter(|node| notes::is_fallback(node, id))
                    .ok_or_else(|| part_error(part, "thread has no matching fallback note"))?;
                if let Operation::Reply(comment) = operation {
                    let reply_id = threads::guid(wb)?;
                    thread_markup =
                        Some(comment.markup(wb, at, &reply_id, Some(id), thread_root)?);
                    new_guid = Some(reply_id);
                    let fallback = threads::fallback_text(part, &entries, &comment.text)?;
                    vec![notes.set_text(current, &fallback)?]
                } else {
                    remove_thread = true;
                    remove_note_shape = true;
                    vec![xml::replace(current, String::new())]
                }
            }
        };
        let removed = if remove_note_shape
            && let Some((part, doc)) = &legacy
            && let Some(node) = vml::note_shape(part, doc, at)?
        {
            Some((part, doc, node))
        } else {
            None
        };
        if matches!(
            added.as_ref().map(|addition| &addition.part),
            Some(vml::Addition::New(_))
        ) {
            wb.pkg
                .xml_part_mut(&sheet)?
                .lens_mut::<sml::Worksheet>()?
                .check_insert_with_ns::<sml::LegacyDrawing>(&REL_NS)?;
        }
        let pkg = &wb.pkg;
        let mut plan = RegistryPlan::new(pkg);
        if let Some(part) = notes_part {
            xml::require_owner(pkg, &part, &sheet)?;
            xml::stage(pkg, &mut plan, &part, &doc, note_edits)?;
        } else {
            let markup = xml::render(&doc, note_edits);
            xml::add(
                pkg,
                &mut plan,
                &sheet,
                REL_NOTES,
                PartNameRequest::Numbered("xl/comments{}.xml".into()),
                NOTES_TYPE,
                markup,
            )?;
        }
        if let Some(markup) = thread_markup {
            threads::append(
                pkg,
                &mut plan,
                &sheet,
                thread.as_ref(),
                entries.last().copied(),
                markup,
            )?;
        }
        let mut retired_guids = Vec::new();
        if remove_thread {
            let (part, doc) = thread.as_ref().expect("thread to remove");
            xml::require_owner(pkg, part, &sheet)?;
            retired_guids.extend(
                entries
                    .iter()
                    .filter_map(|node| node.attr(xml::attr("id")))
                    .map(str::to_ascii_uppercase),
            );
            let edits = entries
                .into_iter()
                .map(|node| xml::replace(node, String::new()))
                .collect();
            xml::stage(pkg, &mut plan, part, doc, edits)?;
        }
        let mut relation = None;
        let mut shape = None;
        let mut block = None;
        if let Some(addition) = added {
            shape = Some(addition.id);
            block = addition.new_block;
            match addition.part {
                vml::Addition::New(markup) => {
                    match plan.add_default_content_type(pkg, "vml", vml::CONTENT_TYPE) {
                        Ok(_)
                        | Err(ooxml_core::Error::RegistryRefused {
                            reason:
                                RegistryRefusal::DefaultExists { .. }
                                | RegistryRefusal::WouldRetypePart { .. },
                            ..
                        }) => {}
                        Err(error) => return Err(error.into()),
                    }
                    relation = Some(xml::add(
                        pkg,
                        &mut plan,
                        &sheet,
                        REL_VML,
                        PartNameRequest::Numbered("xl/drawings/vmlDrawing{}.vml".into()),
                        vml::CONTENT_TYPE,
                        markup,
                    )?);
                }
                vml::Addition::Existing(edits) => {
                    let (part, doc) = legacy.as_ref().expect("existing VML");
                    xml::require_owner(pkg, part, &sheet)?;
                    xml::stage(pkg, &mut plan, part, doc, edits)?;
                }
            }
        }
        if let Some((part, doc, edits)) = updated_box {
            xml::require_owner(pkg, part, &sheet)?;
            xml::stage(pkg, &mut plan, part, doc, edits)?;
        }
        let retired: Vec<_> = if let Some((part, doc, node)) = removed {
            xml::require_owner(pkg, part, &sheet)?;
            xml::stage(
                pkg,
                &mut plan,
                part,
                doc,
                vec![xml::replace(node, String::new())],
            )?;
            [
                xml::attr("id"),
                ooxml_core::QName::new(crate::images::OFFICE, "spid"),
            ]
            .into_iter()
            .filter_map(|name| node.attr(name))
            .filter_map(crate::images::shape_number)
            .collect()
        } else {
            Vec::new()
        };
        wb.apply_registry_edit(plan, |wb| {
            if let Some(rel) = relation {
                wb.fault_point(ApplyStep::NoteDrawing)?;
                wb.pkg
                    .xml_part_mut(&sheet)?
                    .lens_mut::<sml::Worksheet>()?
                    .insert_with_ns::<sml::LegacyDrawing>(&REL_NS)?
                    .set_attr(xml::RID, &rel)?;
            }
            if shape.is_some()
                || !retired.is_empty()
                || new_guid.is_some()
                || !retired_guids.is_empty()
            {
                wb.fault_point(ApplyStep::CommentIndex)?;
                wb.comment_guids
                    .extend(new_guid.into_iter().chain(retired_guids));
                if let Some(block) = block {
                    wb.vml_block_high = Some(block);
                }
                for id in shape.into_iter().chain(retired) {
                    record_shape(wb, &sheet, id);
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
