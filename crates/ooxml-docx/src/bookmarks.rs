//! Bookmark ranges and annotation identifiers (ADR-0011 §5).

use std::fmt::NumBuffer;

use ooxml_core::{ElementSpec, NsDecl, RawNode, RegistryPlan, XmlDocument};
use ooxml_schema::wml::{
    Body, BookmarkEnd, BookmarkStart, Comment, Endnote, Footer, Footnote, Header, Hyperlink, NS_W, Paragraph, ParagraphProperties, Run,
    RunProperties, Table, TableCell, TableRow,
};

use crate::document::{node_at, write_pieces};
use crate::fields::FieldScan;
use crate::names::{ANCHORS, BOOKMARK_END, BOOKMARK_START, NAME, W_ID, bookmark_key, descendants, is, w};
use crate::text::{Piece, map_text};
use crate::{Document, Error, Match, Operation, ParaRef, Result, RunContext};

const W: &[NsDecl<'_>] = &[NsDecl { prefix: "w", uri: NS_W }];

/// An id in Word's annotation registry.
pub type BookmarkId = u32;

/// A text match or a range covering whole paragraphs, inclusive.
#[derive(Clone, Copy, Debug)]
pub enum BookmarkRange<'a> {
    /// A current text match.
    Match(&'a Match),
    /// A paragraph range in one story and container.
    Paragraphs {
        /// First paragraph.
        first: ParaRef,
        /// Last paragraph.
        last: ParaRef,
    },
}

#[derive(Clone, Copy)]
enum Boundary {
    Between(usize),
    Inside { child: usize, byte: usize },
}

struct BookmarkScope {
    max: BookmarkId,
    starts: usize,
    field: bool,
    anchor: bool,
}

impl Document {
    /// Mark a range with a fresh name and id. Text boundaries within a run split that run.
    pub fn add_bookmark(&mut self, name: &str, range: BookmarkRange<'_>) -> Result<BookmarkId> {
        self.write_gate_w1_w3()?;
        let mut chars = name.chars();
        if !chars.next().is_some_and(char::is_alphabetic) || !chars.all(|c| c.is_alphanumeric() || c == '_') || name.chars().count() > 40 {
            return Err(Error::InvalidValue {
                what: "bookmark name",
                value: name.into(),
            });
        }
        let used = self.bookmark_scope(name)?;
        if used.starts > 0 {
            return Err(Error::DuplicateBookmarkName { name: name.into() });
        }
        if used.field {
            return Err(Error::NameReferencedByField { name: name.into() });
        }
        if used.anchor {
            return Err(Error::BookmarkReferenced { name: name.into() });
        }
        self.write_gate_for(Operation::Text)?;
        let id = used
            .max
            .checked_add(1)
            .filter(|id| *id <= i32::MAX as u32)
            .ok_or(Error::AnnotationIdsExhausted)?;
        let (first, last, start, end) = match range {
            BookmarkRange::Match(m) => {
                let p = self.paragraph_at(m.para).ok_or(Error::StaleMatch)?;
                if m.doc != self.id()
                    || m.generation != p.generation()
                    || m.range.is_empty()
                    || p.text().get(m.range.clone()) != Some(m.text.as_str())
                {
                    return Err(Error::StaleMatch);
                }
                let (start, end) = self.match_boundaries(m)?;
                (m.para, m.para, start, end)
            }
            BookmarkRange::Paragraphs { first, last } => {
                let first_p = self.paragraph_at(first).ok_or(Error::NoSuchParagraph(first.index()))?;
                let last_p = self.paragraph_at(last).ok_or(Error::NoSuchParagraph(last.index()))?;
                let after_properties = first_p
                    .raw()
                    .indexed_elements()
                    .find(|(_, n)| is::<ParagraphProperties>(n))
                    .map_or(0, |(i, _)| i + 1);
                (
                    first,
                    last,
                    Boundary::Between(after_properties),
                    Boundary::Between(last_p.raw().children().len()),
                )
            }
        };
        let (first_parent, first_item) = self.para_location(first)?;
        let (last_parent, last_item) = self.para_location(last)?;
        if first.story() != last.story() || first_parent != last_parent || first.index() > last.index() {
            return Err(Error::BookmarkSpansContainers);
        }
        self.check_structure_neighbour(first)?;
        self.check_structure_neighbour(last)?;
        let part = self.stories[first.story().get()].id.part.clone();
        let mut xml = self.pkg.xml_part(&part)?.clone();
        let mut end_path = last_parent;
        end_path.push(last_item);
        let index = split_boundary(&mut xml, &end_path, end)?;
        let mut p = xml.lens_mut_at::<Paragraph>(&end_path)?;
        p.insert_at_with_ns::<BookmarkEnd>(index, W)?
            .set_attr(W_ID, id.format_into(&mut NumBuffer::new()))?;

        let mut start_path = first_parent;
        start_path.push(first_item);
        let index = split_boundary(&mut xml, &start_path, start)?;
        let mut p = xml.lens_mut_at::<Paragraph>(&start_path)?;
        let mut marker = p.insert_at_with_ns::<BookmarkStart>(index, W)?;
        marker.set_attr(W_ID, id.format_into(&mut NumBuffer::new()))?;
        marker.set_attr(NAME, name)?;
        self.save_bookmark_part(&part, xml)?;
        self.annotation_high_water = id;
        self.reindex(first.story());
        self.bump(first);
        if first != last {
            self.bump(last);
        }
        Ok(id)
    }

    /// Remove the uniquely named bookmark. References and ambiguous marker pairs refuse.
    pub fn remove_bookmark(&mut self, name: &str) -> Result<()> {
        self.write_gate_w1_w3()?;
        let used = self.bookmark_scope(name)?;
        if used.starts == 0 {
            return Err(Error::UnknownBookmark { name: name.into() });
        }
        if used.starts != 1 {
            return Err(Error::DuplicateBookmarkName { name: name.into() });
        }
        if used.field || used.anchor {
            return Err(Error::BookmarkReferenced { name: name.into() });
        }
        self.write_gate_for(Operation::Structure)?;

        let folded = bookmark_key(name);
        let invalid = || Error::InvalidBookmark { name: name.into() };
        for part in self.registry_parts(&[])? {
            let story = self.stories().find(|s| s.id().part() == part).map(|s| s.index());
            let xml = self.pkg.xml_part(&part)?;
            let markers = marker_paths(xml.root());
            let Some((start, node)) = markers
                .iter()
                .find(|(_, n)| n.is(BOOKMARK_START) && n.attr(NAME).is_some_and(|n| bookmark_key(n) == folded))
            else {
                continue;
            };
            let id = node.attr(W_ID).and_then(|s| s.trim().parse::<i64>().ok()).ok_or_else(invalid)?;
            let same_id = |n: &RawNode| n.attr(W_ID).and_then(|s| s.trim().parse().ok()) == Some(id);
            let mut ends = markers.iter().filter(|(_, n)| n.is(BOOKMARK_END) && same_id(n));
            let (end, _) = ends.next().ok_or_else(invalid)?;
            if start >= end || ends.next().is_some() || markers.iter().filter(|(_, n)| n.is(BOOKMARK_START) && same_id(n)).count() != 1 {
                return Err(invalid());
            }
            let affected = story.map_or_default(|story| {
                let index = &self.stories[story.get()].index;
                index
                    .paragraphs
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        let path = index.path(p.frame);
                        (start.starts_with(&path) || end.starts_with(&path)).then_some(ParaRef::new(story, i))
                    })
                    .collect::<Vec<_>>()
            });
            let mut edited = xml.clone();
            remove_marker(&mut edited, end)?;
            remove_marker(&mut edited, start)?;
            self.save_bookmark_part(&part, edited)?;
            self.annotation_high_water = used.max;
            if let Some(story) = story {
                self.reindex(story);
                for para in affected {
                    self.bump(para);
                }
            }
            return Ok(());
        }
        Err(invalid())
    }

    fn bookmark_scope(&mut self, name: &str) -> Result<BookmarkScope> {
        let name = bookmark_key(name);
        let mut used = BookmarkScope {
            max: self.annotation_high_water,
            starts: 0,
            field: false,
            anchor: false,
        };
        for part in self.registry_parts(&[])? {
            let root = self.pkg.xml_part(&part)?.root();
            used.field |= FieldScan::bookmark_targets(root).contains(&name);
            for node in descendants(root).filter(|n| n.name().ns() == NS_W) {
                let local = node.name().local();
                if (ANCHORS.contains(&local)
                    || matches!(
                        local,
                        "commentReference"
                            | "comment"
                            | "ins"
                            | "del"
                            | "moveFrom"
                            | "moveTo"
                            | "rPrChange"
                            | "pPrChange"
                            | "sectPrChange"
                            | "tblPrChange"
                            | "trPrChange"
                            | "tcPrChange"
                            | "tblGridChange"
                            | "numberingChange"
                            | "cellIns"
                            | "cellDel"
                            | "cellMerge"
                    ))
                    && let Some(id) = node.attr(W_ID).and_then(|s| s.trim().parse::<u64>().ok())
                {
                    used.max = used.max.max(BookmarkId::try_from(id).unwrap_or(BookmarkId::MAX));
                }
                if node.is(BOOKMARK_START) && node.attr(NAME).is_some_and(|n| bookmark_key(n) == name) {
                    used.starts += 1;
                }
                used.anchor |= is::<Hyperlink>(node) && node.attr(w("anchor")).is_some_and(|n| bookmark_key(n) == name);
            }
        }
        Ok(used)
    }

    fn match_boundaries(&self, m: &Match) -> Result<(Boundary, Boundary)> {
        let p = self.paragraph_at(m.para).ok_or(Error::StaleMatch)?;
        let index = &self.stories[m.para.story().get()].index;
        let mut offset = 0;
        let mut start = None;
        let mut end = None;
        for run in p.runs().filter(|r| !r.is_removed()) {
            let past = offset + run.text().len();
            if m.range.start < past && m.range.end > offset {
                if run.context() != RunContext::Direct {
                    return Err(Error::UnsupportedContext {
                        ancestor: run.wrappers()[0].qname(),
                    });
                }
                run.check_settable()?;
                let path = index.path(run.info.frame);
                let child = *path.last().expect("run path");
                if start.is_none() {
                    start = Some(if offset == m.range.start {
                        Boundary::Between(child)
                    } else {
                        Boundary::Inside {
                            child,
                            byte: m.range.start - offset,
                        }
                    });
                }
                if m.range.end <= past {
                    end = Some(if past == m.range.end {
                        Boundary::Between(child + 1)
                    } else {
                        Boundary::Inside {
                            child,
                            byte: m.range.end - offset,
                        }
                    });
                    break;
                }
            }
            offset = past;
        }
        start.zip(end).ok_or(Error::StaleMatch)
    }

    fn save_bookmark_part(&mut self, part: &str, xml: XmlDocument) -> Result<()> {
        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, part, xml)?;
        self.pkg.apply(plan)?;
        Ok(())
    }
}

fn split_boundary(xml: &mut XmlDocument, path: &[usize], boundary: Boundary) -> Result<usize> {
    let (child, byte) = match boundary {
        Boundary::Between(index) => return Ok(index),
        Boundary::Inside { child, byte } => (child, byte),
    };
    let mut run_path = path.to_vec();
    run_path.push(child);
    let run = xml.lens_at::<Run>(&run_path)?;
    let text = crate::text::run_text(run.raw());
    // A literal CR in a text node would be normalized by the run writer.
    if text.contains('\r') {
        return Err(Error::RunNotTextOnly {
            child: "literal carriage return".into(),
        });
    }
    let rpr = run
        .child::<RunProperties>()
        .map(|n| {
            if n.raw().elements().any(|n| n.is(crate::names::RPR_CHANGE)) {
                return Err(Error::TrackedPropertyChange { element: "w:rPrChange" });
            }
            Ok(n.detach_as::<RunProperties>(&[])?)
        })
        .transpose()?;
    let mut prefix = xml.lens_mut_at::<Run>(&run_path)?;
    write_pieces(&mut prefix, &split_text_pieces(&text[..byte])?)?;
    let mut p = xml.lens_mut_at::<Paragraph>(path)?;
    let mut suffix = p.insert_at_with_ns::<Run>(child + 1, W)?;
    if let Some(rpr) = rpr {
        suffix.insert_detached(&rpr)?;
    }
    write_pieces(&mut suffix, &split_text_pieces(&text[byte..])?)?;
    Ok(child + 1)
}

fn split_text_pieces(text: &str) -> Result<Vec<Piece>> {
    let mut pieces = Vec::new();
    for segment in text.split_inclusive('\r') {
        // CR in a run's read model comes from a literal text character.
        // Leave it as text, so the XML writer escapes it as &#13;.
        if let Some(prefix) = segment.strip_suffix('\r') {
            pieces.extend(map_text(prefix)?);
            pieces.push(Piece::Text("\r".into()));
        } else {
            pieces.extend(map_text(segment)?);
        }
    }
    Ok(pieces)
}

fn marker_paths(root: &RawNode) -> Vec<(Vec<usize>, &RawNode)> {
    let mut found = Vec::new();
    let mut stack = vec![(Vec::new(), root)];
    while let Some((path, node)) = stack.pop() {
        if node.is(BOOKMARK_START) || node.is(BOOKMARK_END) {
            found.push((path.clone(), node));
        }
        stack.extend(node.indexed_elements().map(|(index, child)| {
            let mut child_path = path.clone();
            child_path.push(index);
            (child_path, child)
        }));
    }
    found
}

fn remove_marker(xml: &mut XmlDocument, path: &[usize]) -> Result<()> {
    let (&index, parent) = path.split_last().expect("a bookmark has a parent");
    let node = node_at(xml.root(), parent);
    macro_rules! remove {
        ($($typ:ty),*) => {
            match node.name().local() {
                $(local if local == <$typ>::QNAME.local => {
                    xml.lens_mut_at::<$typ>(parent)?.remove_at(index);
                    Ok(())
                })*
                _ => Err(Error::UnsupportedContext { ancestor: "bookmark parent" }),
            }
        };
    }
    remove!(
        Paragraph, Body, Header, Footer, Footnote, Endnote, Comment, Table, TableRow, TableCell, Hyperlink
    )
}
