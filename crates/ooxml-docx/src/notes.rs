//! Atomic insertion of footnotes and endnotes (ADR-0015 §4).

use std::{collections::BTreeSet, fmt::NumBuffer};

use ooxml_core::{
    ChildOf, ElementSpec, LensMut, NsDecl, PartNameRequest, QName, RegistryPlan, Target,
    XmlDocument,
};
use ooxml_schema::wml::*;

use crate::document::write_pieces;
use crate::names::descendants;
use crate::text::{Piece, map_text};
use crate::{Document, Error, Operation, Result, RunPosition, StoryKind};

const W: &[NsDecl<'_>] = &[NsDecl {
    prefix: "w",
    uri: NS_W,
}];
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CONTENT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}

/// The id of a footnote or endnote, in its own kind's id space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteId(pub i32);

fn styled_run<'a>(
    p: &'a mut LensMut<'_, Paragraph>,
    after: Option<usize>,
    style: &str,
) -> Result<LensMut<'a, Run>> {
    let mut run = if let Some(after) = after {
        p.insert_at_with_ns::<Run>(after + 1, W)?
    } else {
        p.insert_with_ns::<Run>(W)?
    };
    run.insert::<RunProperties>()?
        .insert::<RunStyle>()?
        .set_attr(w("val"), style)?;
    Ok(run)
}

fn make_note<R, N, M>(
    xml: &mut XmlDocument,
    id: i32,
    paragraphs: &[Vec<Piece>],
    body_style: &str,
    reference_style: &str,
) -> Result<()>
where
    R: ElementSpec,
    N: ChildOf<R>,
    Paragraph: ChildOf<N>,
    M: ChildOf<Run>,
{
    let mut root = xml.lens_mut::<R>()?;
    let mut note = root.insert_with_ns::<N>(W)?;
    note.set_attr(w("id"), id.format_into(&mut NumBuffer::new()))?;
    for (index, pieces) in paragraphs.iter().enumerate() {
        let mut p = note.insert::<Paragraph>()?;
        p.insert::<ParagraphProperties>()?
            .insert::<ParagraphStyleId>()?
            .set_attr(w("val"), body_style)?;
        if index == 0 {
            styled_run(&mut p, None, reference_style)?.insert::<M>()?;
        }
        write_pieces(&mut p.insert::<Run>()?, pieces)?;
    }
    Ok(())
}

const SEPARATORS: [(&str, &str); 2] = [("separator", "-1"), ("continuationSeparator", "0")];

fn add_separators<R, N>(xml: &mut XmlDocument) -> Result<()>
where
    R: ElementSpec,
    N: ChildOf<R>,
    Paragraph: ChildOf<N>,
{
    let mut root = xml.lens_mut::<R>()?;
    for (kind, id) in SEPARATORS {
        let mut note = root.insert::<N>()?;
        note.set_attr(w("type"), kind)?;
        note.set_attr(w("id"), id)?;
        let mut p = note.insert::<Paragraph>()?;
        let mut r = p.insert::<Run>()?;
        if id == "-1" {
            r.insert::<SeparatorMark>()?;
        } else {
            r.insert::<ContinuationSeparatorMark>()?;
        }
    }
    Ok(())
}

impl Document {
    /// Adds a footnote at a paragraph boundary or after a direct text run.
    pub fn add_footnote(&mut self, at: RunPosition, paragraphs: Vec<String>) -> Result<NoteId> {
        self.add_note(at, &paragraphs, false)
    }

    /// Adds an endnote at a paragraph boundary or after a direct text run.
    pub fn add_endnote(&mut self, at: RunPosition, paragraphs: Vec<String>) -> Result<NoteId> {
        self.add_note(at, &paragraphs, true)
    }

    fn related_note_part(&self, name: &str) -> Result<Option<String>> {
        let rels = self.pkg.relationships(self.main_part_name())?;
        let rel_type = format!("{REL}/{name}");
        let mut candidates = rels.by_type(&rel_type);
        let Some(first) = candidates.next() else {
            return Ok(None);
        };
        if candidates.next().is_some() {
            return Err(Error::InvalidNotePart);
        }
        Ok(Some(
            rels.resolve(first)
                .filter(|name| self.pkg.contains(name))
                .ok_or(Error::InvalidNotePart)?,
        ))
    }

    fn new_note_xml(&self, local: &str) -> Result<XmlDocument> {
        Ok(XmlDocument::parse_with(
            format!(r#"<w:{local} xmlns:w="{NS_W}"/>"#).as_bytes(),
            &self.pkg.parse_options(),
        )?)
    }

    fn stage_note_xml(
        &self,
        plan: &mut RegistryPlan,
        local: &str,
        part: Option<String>,
        xml: XmlDocument,
    ) -> Result<String> {
        if let Some(part) = part {
            plan.stage_part(&self.pkg, &part, xml)?;
            return Ok(part);
        }

        let content_type = format!("{CONTENT}.{local}+xml");
        let name = match plan.add_part(
            &self.pkg,
            PartNameRequest::Exact(format!("word/{local}.xml")),
            &content_type,
            xml.clone(),
        ) {
            Err(ooxml_core::Error::RegistryRefused {
                reason: ooxml_core::RegistryRefusal::NameInUse { .. },
                ..
            }) => plan.add_part(
                &self.pkg,
                PartNameRequest::Numbered(format!("word/{local}{{}}.xml")),
                &content_type,
                xml,
            )?,
            result => result?,
        };
        let id = plan.add_relationship(
            &self.pkg,
            Some(self.main_part_name()),
            &format!("{REL}/{local}"),
            Target::Part(name.clone()),
        )?;
        if descendants(self.story_root_node(crate::StoryIndex::MAIN)).any(|node| {
            node.attrs()
                .iter()
                .any(|a| a.name().ns() == NS_R && a.value().eq_ignore_ascii_case(&id))
        }) {
            return Err(Error::RelIdReferenced { id });
        }
        Ok(name)
    }

    fn add_note(
        &mut self,
        at: RunPosition,
        paragraphs: &[String],
        endnote: bool,
    ) -> Result<NoteId> {
        self.write_gate_for(Operation::Structure)?;
        let para = at.para();
        self.check_structure_neighbour(para)?;
        let after = if let RunPosition::After(reference) = at {
            let run = self.run_at(reference).ok_or(Error::NoSuchRun {
                paragraph: para.index,
                run: reference.index(),
            })?;
            run.check_settable()?;
            if run.has_formatting_change() {
                return Err(Error::TrackedPropertyChange {
                    element: "w:rPrChange",
                });
            }
            let run_path = self.stories[para.story.0].index.path(run.info.frame);
            let p = self
                .paragraph_at(para)
                .ok_or(Error::NoSuchParagraph(para.index))?;
            let para_path = self.stories[para.story.0].index.path(p.info.frame);
            if run_path.len() != para_path.len() + 1 {
                return Err(Error::NestedHyperlink);
            }
            run_path.last().copied()
        } else {
            None
        };

        let (mut path, item) = self.para_location(para)?;
        path.push(item);

        if paragraphs.is_empty() {
            return Err(Error::InvalidValue {
                what: "note paragraphs",
                value: String::new(),
            });
        }
        let pieces = paragraphs
            .iter()
            .enumerate()
            .map(|(index, text)| {
                if index == 0 {
                    map_text(&format!(" {text}"))
                } else {
                    map_text(text)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let target = self.stories[para.story.0].id.part.clone();
        let mut source = self.pkg.xml_part(&target)?.clone();
        let mut parts = [
            self.related_note_part("footnotes")?,
            self.related_note_part("endnotes")?,
        ];
        for (part, expected) in parts.iter().zip([Footnotes::QNAME, Endnotes::QNAME]) {
            if let Some(part) = part
                && !self.pkg.xml_part(part)?.root().is(expected)
            {
                return Err(Error::InvalidNotePart);
            }
        }
        let kind = usize::from(endnote);
        let (definition, reference, body_name, ref_name, body_id, ref_id) = if endnote {
            (
                "endnote",
                "endnoteReference",
                "endnote text",
                "endnote reference",
                "EndnoteText",
                "EndnoteReference",
            )
        } else {
            (
                "footnote",
                "footnoteReference",
                "footnote text",
                "footnote reference",
                "FootnoteText",
                "FootnoteReference",
            )
        };

        let mut high = self.note_high_water[kind];
        for part in self.registry_parts(&[])? {
            let root = self.pkg.xml_part(&part)?.root();
            for node in descendants(root) {
                if (node.is(w(reference)) || node.is(w(definition)))
                    && let Some(id) = node.attr(w("id"))
                {
                    high = high.max(id.parse::<i32>().map_err(|_| Error::InvalidNotePart)?);
                }
            }
        }
        let id = high.checked_add(1).ok_or(Error::NoteIdsExhausted)?;

        if self.styles_ambiguous {
            return Err(Error::AmbiguousStylesPart);
        }
        let styles_part = self.styles_part.clone();
        let mut styles = match &styles_part {
            Some(part) => self.pkg.xml_part(part)?.clone(),
            None => self.new_note_xml("styles")?,
        };
        let mut used = self.used_style_ids()?;
        let (body_style, body_added) =
            note_style(&mut styles, &mut used, body_name, body_id, "paragraph")?;
        let (ref_style, ref_added) =
            note_style(&mut styles, &mut used, ref_name, ref_id, "character")?;

        let mut p = source.lens_mut_at::<Paragraph>(&path)?;
        let mut run = styled_run(&mut p, after, &ref_style)?;
        let mut num = NumBuffer::new();
        let id_text = id.format_into(&mut num);
        if endnote {
            run.insert::<EndnoteReference>()?
                .set_attr(w("id"), id_text)?;
        } else {
            run.insert::<FootnoteReference>()?
                .set_attr(w("id"), id_text)?;
        }

        let mut note_docs = [None, None];
        for (i, local) in ["footnotes", "endnotes"].into_iter().enumerate() {
            if i != kind && parts[i].is_some() {
                continue;
            }
            let mut xml = match &parts[i] {
                Some(part) if *part == target => source.clone(),
                Some(part) => self.pkg.xml_part(part)?.clone(),
                None => {
                    let mut xml = self.new_note_xml(local)?;
                    if i == 0 {
                        add_separators::<Footnotes, Footnote>(&mut xml)?;
                    } else {
                        add_separators::<Endnotes, Endnote>(&mut xml)?;
                    }
                    xml
                }
            };
            if i == kind {
                if endnote {
                    make_note::<Endnotes, Endnote, EndnoteReferenceMark>(
                        &mut xml,
                        id,
                        &pieces,
                        &body_style,
                        &ref_style,
                    )?;
                } else {
                    make_note::<Footnotes, Footnote, FootnoteReferenceMark>(
                        &mut xml,
                        id,
                        &pieces,
                        &body_style,
                        &ref_style,
                    )?;
                }
            }
            note_docs[i] = Some(xml);
        }

        let settings = if parts.iter().any(Option::is_none) {
            if self.settings_parts.len() > 1 {
                return Err(Error::AmbiguousSettings);
            }
            let part = self.settings_parts.first().cloned();
            let mut xml = match &part {
                Some(part) => self.pkg.xml_part(part)?.clone(),
                None => self.new_note_xml("settings")?,
            };
            let separator_ids = |index: usize| {
                let Some(part) = &parts[index] else {
                    return SEPARATORS.map(|(_, id)| id).to_vec();
                };
                let root = self.pkg.parsed_part(part).expect("parsed note part").root();
                let name = if index == 0 { "footnote" } else { "endnote" };
                root.elements()
                    .filter(|node| {
                        node.is(w(name))
                            && matches!(
                                node.attr(w("type")),
                                Some("separator" | "continuationSeparator")
                            )
                    })
                    .filter_map(|node| node.attr(w("id")))
                    .collect()
            };
            let mut settings = xml.lens_mut::<Settings>()?;
            settings_separators::<FootnoteDocumentWideProperties, FootnoteSpecialReference>(
                &mut settings,
                &separator_ids(0),
            )?;
            settings_separators::<EndnoteDocumentWideProperties, EndnoteSpecialReference>(
                &mut settings,
                &separator_ids(1),
            )?;
            Some((part, xml))
        } else {
            None
        };

        let mut plan = RegistryPlan::new(&self.pkg);
        if parts[kind].as_deref() != Some(&target) {
            plan.stage_part(&self.pkg, &target, source)?;
        }
        for (i, local) in ["footnotes", "endnotes"].into_iter().enumerate() {
            if let Some(xml) = note_docs[i].take() {
                parts[i] = Some(self.stage_note_xml(&mut plan, local, parts[i].take(), xml)?);
            }
        }
        let styles_part = if body_added || ref_added {
            Some(self.stage_note_xml(&mut plan, "styles", styles_part, styles)?)
        } else {
            styles_part
        };
        let settings_part = settings
            .map(|(part, xml)| self.stage_note_xml(&mut plan, "settings", part, xml))
            .transpose()?;
        self.pkg.apply(plan)?;
        self.styles_part = styles_part;
        if self.settings_parts.is_empty() {
            self.settings_parts.extend(settings_part);
        }
        for (part, story_kind) in parts
            .into_iter()
            .zip([StoryKind::Footnotes, StoryKind::Endnotes])
        {
            self.refresh_note_story(story_kind, part.expect("both note parts staged"));
        }
        self.reindex(para.story);
        self.bump(para);
        self.note_high_water[kind] = id;
        Ok(NoteId(id))
    }
}

fn settings_separators<P, S>(settings: &mut LensMut<'_, Settings>, ids: &[&str]) -> Result<()>
where
    P: ChildOf<Settings>,
    S: ChildOf<P>,
{
    let mut props = settings.get_or_insert_with_ns::<P>(W)?;
    for &id in ids {
        if !props
            .as_lens()
            .raw()
            .elements()
            .any(|node| node.is(S::QNAME) && node.attr(w("id")) == Some(id))
        {
            props.insert::<S>()?.set_attr(w("id"), id)?;
        }
    }
    Ok(())
}

fn note_style(
    xml: &mut XmlDocument,
    used: &mut BTreeSet<String>,
    name: &str,
    stem: &str,
    kind: &str,
) -> Result<(String, bool)> {
    let mut styles = xml.lens_mut::<Styles>()?;
    let view = styles.as_lens();
    let existing = || view.raw().elements().filter(|node| node.is(Style::QNAME));
    let mut named = existing().filter(|node| {
        node.attr(w("type")).unwrap_or("paragraph") == kind
            && node.elements().any(|child| {
                child.is(StyleName::QNAME)
                    && child
                        .attr(w("val"))
                        .is_some_and(|value| value.eq_ignore_ascii_case(name))
            })
    });
    if let Some(style) = named.next() {
        let id = style
            .attr(w("styleId"))
            .filter(|id| !id.is_empty())
            .ok_or(Error::InvalidNotePart)?;
        if named.next().is_some()
            || existing()
                .filter(|node| {
                    node.attr(w("styleId"))
                        .is_some_and(|other| other.eq_ignore_ascii_case(id))
                })
                .count()
                != 1
        {
            return Err(Error::AmbiguousStyle { id: id.into() });
        }
        return Ok((id.into(), false));
    }
    drop(named);
    let id = std::iter::once(stem.to_owned())
        .chain((1..).map(|i| format!("{stem}{i}")))
        .find(|id| used.insert(id.to_ascii_lowercase()))
        .expect("finite registry");
    let mut style = styles.insert_with_ns::<Style>(W)?;
    style.set_attr(w("type"), kind)?;
    style.set_attr(w("styleId"), &id)?;
    style.insert::<StyleName>()?.set_attr(w("val"), name)?;
    Ok((id, true))
}
