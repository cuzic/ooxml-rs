//! Construction and cached values for complex fields (ADR-0015 §7).

use std::fmt::NumBuffer;

use chrono::Local;
use ooxml_core::name::XML_SPACE;
use ooxml_core::{LensMut, NsDecl, QName, RawNode, RegistryPlan, XmlDocument};
use ooxml_schema::wml::{FieldChar, FieldCode, NS_W, Paragraph, Run, RunProperties};

use crate::controls::date;
use crate::document::write_pieces;
use crate::fields::{FieldScan, QUOTES, normalize_name};
use crate::names::{bookmark_key, w};
use crate::text::{map_text, run_child_text};
use crate::{Document, Error, Operation, Result, RunContext, RunPosition};

const W: &[NsDecl<'_>] = &[NsDecl {
    prefix: "w",
    uri: NS_W,
}];
const TOC_PLACEHOLDER: &str = "Table of contents — update this field.";

const CORE_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const EXTENDED_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CORE_NS: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const EXTENDED_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

/// A complex Word field supported by [Document::insert_field].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldSpec {
    /// Current page number.
    Page,
    /// Total page count.
    NumPages,
    /// Page count of the current section.
    SectionPages,
    /// Current local date, using a Word date/time picture.
    Date { format: String },
    /// Current local time, using a Word date/time picture.
    Time { format: String },
    /// A property allowed by the document's W2 classifier.
    /// Date-valued properties have an empty cache and are marked dirty for Word.
    DocProperty { name: String },
    /// Text of an existing bookmark, optionally hyperlinked.
    /// Insertion inside the target bookmark is refused.
    Ref { bookmark: String, hyperlink: bool },
    /// Page containing an existing bookmark, optionally hyperlinked.
    PageRef { bookmark: String, hyperlink: bool },
    /// Number of preceding fields with this identifier in the story, plus one.
    /// The identifier starts with a Unicode letter and may contain Unicode
    /// letters, numbers, and underscores. The cache ignores earlier SEQ
    /// switches such as `\r`, `\c`, and `\h`. The field is marked dirty for Word
    /// to update.
    Seq { identifier: String },
    /// Table of contents, cached as a placeholder in the target paragraph.
    /// The inclusive heading levels must be in 1–9.
    Toc { levels: (u8, u8), hyperlinks: bool },
}

impl FieldSpec {
    fn page_code(&self) -> Option<&'static str> {
        match self {
            Self::Page => Some("PAGE"),
            Self::NumPages => Some("NUMPAGES"),
            Self::SectionPages => Some("SECTIONPAGES"),
            _ => None,
        }
    }

    fn dirty_code(&self) -> Option<&'static str> {
        match self {
            Self::Ref { .. } => Some("REF"),
            Self::PageRef { .. } => Some("PAGEREF"),
            Self::Seq { .. } => Some("SEQ"),
            Self::Toc { .. } => Some("TOC"),
            _ => None,
        }
    }
}

fn quote(argument: &str) -> Result<String> {
    if argument.trim().is_empty()
        || argument.contains(QUOTES)
        || argument.contains('\\')
        || argument.chars().any(char::is_control)
    {
        return Err(Error::InvalidValue {
            what: "field argument",
            value: argument.into(),
        });
    }
    Ok(format!("\"{argument}\""))
}

fn hyperlink_switch(enabled: bool) -> &'static str {
    if enabled { r" \h" } else { "" }
}

impl Document {
    /// Inserts a complex field with a default cache of 1 for pagination.
    /// Use [Self::insert_field_with_page_cache] to supply a different value.
    ///
    /// Content-dependent fields require a session opt-in via
    /// [Self::enable_update_fields_on_open]. They are marked dirty and
    /// participate in the usual stale-field gate on reopening the document.
    /// All five runs copy the anchor run's formatting. At paragraph end they
    /// use the last element when that element is a direct, unrevised run.
    /// Paragraphs inside block-level w:sdt or w:customXml return
    /// [Error::UnsupportedContext].
    pub fn insert_field(&mut self, at: RunPosition, f: FieldSpec) -> Result<()> {
        self.insert_field_impl(at, f, None)
    }

    /// Inserts a pagination field with the caller's cached page number or
    /// page count. The field must be Page, NumPages or SectionPages.
    pub fn insert_field_with_page_cache(
        &mut self,
        at: RunPosition,
        f: FieldSpec,
        cache: u32,
    ) -> Result<()> {
        self.insert_field_impl(at, f, Some(cache))
    }

    fn insert_field_impl(
        &mut self,
        at: RunPosition,
        f: FieldSpec,
        page_cache: Option<u32>,
    ) -> Result<()> {
        self.write_gate_for(Operation::Text)?;
        if page_cache.is_some() && f.page_code().is_none() {
            return Err(Error::InvalidValue {
                what: "pagination field",
                value: format!("{f:?}"),
            });
        }
        let dirty = f.dirty_code();
        if let Some(code) = dirty
            && !(self.fields_opt_in && self.update_fields_on_open() == Some(true))
        {
            return Err(Error::StaleFieldRisk {
                codes: vec![code.into()],
            });
        }

        let at_para = at.para();
        let (mut path, item) = self.para_location(at_para)?;
        path.push(item);
        let p = self
            .paragraph_at(at_para)
            .ok_or(Error::NoSuchParagraph(at_para.index))?;
        self.check_paragraph_mark(at_para, &p, false)?;
        if p.info.in_revision_range
            || p.raw()
                .elements()
                .any(|node| crate::revisions::range_marker(node).is_some())
        {
            return Err(Error::InsideRevision {
                context: RunContext::Direct,
            });
        }
        let root = self.story_root_node(at_para.story);
        crate::revisions::check_path(root, &path)?;
        let data = &self.stories[at_para.story.0];
        let (index, anchor_path) = match at {
            RunPosition::After(reference) => {
                let run = self.run_at(reference).ok_or(Error::NoSuchRun {
                    paragraph: at_para.index,
                    run: reference.index(),
                })?;
                crate::revisions::check_run_revision(&run)?;
                if let Some(wrapper) = run.wrappers().first() {
                    return Err(Error::UnsupportedContext {
                        ancestor: wrapper.qname(),
                    });
                }
                let run_path = data.index.path(run.info.frame);
                (run_path[path.len()] + 1, Some(run_path))
            }
            RunPosition::EndOf(_) => {
                let anchor = p.raw().indexed_elements().last().and_then(|(item, node)| {
                    (node.is(w("r"))
                        && !node
                            .elements()
                            .any(|e| e.is(w("rPr")) && e.elements().any(|n| n.is(w("rPrChange")))))
                    .then(|| {
                        let mut run_path = path.clone();
                        run_path.push(item);
                        run_path
                    })
                });
                (p.raw().children().len(), anchor)
            }
        };
        let sequence = match &f {
            FieldSpec::Seq { identifier } => Some(identifier.as_str()),
            _ => None,
        };
        let (inside_field, preceding) = FieldScan::before_insertion(root, p.raw(), index, sequence);
        if inside_field {
            return Err(Error::InsideField);
        }
        let (instruction, cached, refresh_property) =
            self.field_value(f, page_cache.unwrap_or(1), preceding, (p.raw(), index))?;
        let pieces = map_text(&cached)?;
        let part = data.id.part.clone();
        let mut xml = self.pkg.parsed_part(&part).expect("parsed story").clone();
        let properties = anchor_path
            .map(|anchor| xml.lens_at::<Run>(&anchor))
            .transpose()?
            .and_then(|run| run.child::<RunProperties>())
            .map(|properties| properties.detach_as::<RunProperties>(&[]))
            .transpose()?;
        let mut p = xml.lens_mut_at::<Paragraph>(&path)?;
        for offset in 0..5 {
            let mut run = p.insert_at_with_ns::<Run>(index + offset, W)?;
            if let Some(properties) = &properties {
                run.insert_detached(properties)?;
            }
            match offset {
                0 => marker(&mut run, "begin", dirty.is_some() || refresh_property)?,
                1 => {
                    let mut code = run.insert::<FieldCode>()?;
                    code.set_attr(XML_SPACE, "preserve")?;
                    code.set_text(&instruction)?;
                }
                2 => marker(&mut run, "separate", false)?,
                3 => write_pieces(&mut run, &pieces)?,
                4 => marker(&mut run, "end", false)?,
                _ => unreachable!(),
            }
        }

        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, &part, xml)?;
        self.pkg.apply(plan)?;
        self.reindex(at_para.story);
        self.bump(at_para);
        Ok(())
    }

    fn field_value(
        &self,
        f: FieldSpec,
        page_cache: u32,
        preceding: usize,
        insertion: (&RawNode, usize),
    ) -> Result<(String, String, bool)> {
        if let Some(code) = f.page_code() {
            return Ok((
                format!(" {code} "),
                page_cache.format_into(&mut NumBuffer::new()).into(),
                false,
            ));
        }
        let pair = match f {
            FieldSpec::Date { ref format } | FieldSpec::Time { ref format } => {
                let code = if matches!(f, FieldSpec::Date { .. }) {
                    "DATE"
                } else {
                    "TIME"
                };
                let argument = quote(format)?;
                let cached = date::format(Local::now().naive_local(), format, "en-US")?;
                (format!(r#" {code} \@ {argument} "#), cached)
            }
            FieldSpec::DocProperty { name } => {
                let instruction = format!(" DOCPROPERTY {} ", quote(&name)?);
                let key = normalize_name(&name);
                let custom = self.custom_field_property(&instruction, &key)?;
                let (cached, is_date) = match custom {
                    Some(value) => value,
                    None => self.builtin_field_property(&key)?,
                };
                return Ok((instruction, cached, is_date));
            }
            FieldSpec::Ref {
                ref bookmark,
                hyperlink,
            }
            | FieldSpec::PageRef {
                ref bookmark,
                hyperlink,
            } => {
                let code = f.dirty_code().expect("reference field");
                let target = quote(bookmark)?;
                let text =
                    self.field_bookmark_text(bookmark, (code == "REF").then_some(insertion))?;
                let cached = if code == "REF" { text } else { "?".into() };
                (
                    format!(" {code} {target}{} ", hyperlink_switch(hyperlink)),
                    cached,
                )
            }
            FieldSpec::Seq { identifier } => {
                let mut chars = identifier.chars();
                if !chars.next().is_some_and(char::is_alphabetic)
                    || !chars.all(|ch| ch.is_alphanumeric() || ch == '_')
                {
                    return Err(Error::InvalidValue {
                        what: "SEQ identifier",
                        value: identifier,
                    });
                }
                (
                    format!(" SEQ \"{identifier}\" "),
                    (preceding + 1).format_into(&mut NumBuffer::new()).into(),
                )
            }
            FieldSpec::Toc {
                levels: (first, last),
                hyperlinks,
            } => {
                if first == 0 || first > last || last > 9 {
                    return Err(Error::InvalidValue {
                        what: "TOC heading levels",
                        value: format!("{first}-{last}"),
                    });
                }
                (
                    format!(
                        r#" TOC \o "{first}-{last}"{} "#,
                        hyperlink_switch(hyperlinks)
                    ),
                    TOC_PLACEHOLDER.into(),
                )
            }
            FieldSpec::Page | FieldSpec::NumPages | FieldSpec::SectionPages => {
                unreachable!("pagination fields handled above")
            }
        };
        Ok((pair.0, pair.1, false))
    }

    fn field_bookmark_text(
        &self,
        name: &str,
        insertion: Option<(&RawNode, usize)>,
    ) -> Result<String> {
        let key = bookmark_key(name);
        for story in self.stories() {
            let mut nodes = vec![Some(story.raw())];
            let mut id = None;
            let mut contains_insertion = false;
            let mut text = String::new();
            while let Some(node) = nodes.pop() {
                let Some(node) = node else {
                    contains_insertion = id.is_some();
                    continue;
                };
                if node.is(w("del")) || node.is(w("moveFrom")) {
                    continue;
                }
                if let Some(id) = id {
                    if node.is(w("bookmarkEnd")) && node.attr(w("id")) == Some(id) {
                        if contains_insertion {
                            return Err(Error::InvalidValue {
                                what: "self-referencing REF bookmark",
                                value: name.into(),
                            });
                        }
                        return Ok(text);
                    }
                    if node.is(w("p")) {
                        text.push('\n');
                    } else if !node.is(w("delText")) {
                        text.push_str(&run_child_text(node));
                    }
                } else if node.is(w("bookmarkStart"))
                    && node
                        .attr(w("name"))
                        .is_some_and(|value| bookmark_key(value) == key)
                {
                    id = node.attr(w("id"));
                }
                let children = node.children();
                let boundary = insertion.filter(|(paragraph, _)| std::ptr::eq(node, *paragraph));
                let (before, after) =
                    children.split_at(boundary.map_or(children.len(), |(_, i)| i));
                nodes.extend(
                    after
                        .iter()
                        .rev()
                        .filter_map(|item| item.as_element())
                        .map(Some),
                );
                if boundary.is_some() {
                    // Visit the insertion boundary between its adjacent children.
                    nodes.push(None);
                }
                nodes.extend(
                    before
                        .iter()
                        .rev()
                        .filter_map(|item| item.as_element())
                        .map(Some),
                );
            }
        }
        Err(Error::UnknownBookmark { name: name.into() })
    }

    fn builtin_field_property(&self, name: &str) -> Result<(String, bool)> {
        let (ns, local) = match name {
            "AUTHOR" => (DC_NS, "creator"),
            "CATEGORY" => (CORE_NS, "category"),
            "COMMENTS" => (DC_NS, "description"),
            "COMPANY" => (EXTENDED_NS, "Company"),
            "HYPERLINKBASE" => (EXTENDED_NS, "HyperlinkBase"),
            "KEYWORDS" => (CORE_NS, "keywords"),
            "LASTSAVEDBY" => (CORE_NS, "lastModifiedBy"),
            "MANAGER" => (EXTENDED_NS, "Manager"),
            "NAMEOFAPPLICATION" => (EXTENDED_NS, "Application"),
            "PAGES" => (EXTENDED_NS, "Pages"),
            "REVISIONNUMBER" => (CORE_NS, "revision"),
            "SECURITY" => (EXTENDED_NS, "DocSecurity"),
            "SUBJECT" => (DC_NS, "subject"),
            "TEMPLATE" => (EXTENDED_NS, "Template"),
            "TITLE" => (DC_NS, "title"),
            "TOTALEDITINGTIME" => (EXTENDED_NS, "TotalTime"),
            "CREATETIME" | "LASTSAVEDTIME" | "LASTPRINTED" => return Ok((String::new(), true)),
            _ => return Ok((String::new(), false)),
        };
        let relationship = if ns == EXTENDED_NS {
            EXTENDED_REL
        } else {
            CORE_REL
        };
        let rels = self.pkg.package_relationships();
        for part in rels
            .by_type(relationship)
            .filter_map(|rel| rels.resolve(rel))
            .filter(|part| self.pkg.contains(part))
        {
            let xml = XmlDocument::parse(&self.pkg.read_part(&part)?)?;
            if let Some(value) = xml
                .root()
                .elements()
                .find(|node| node.is(QName::new(ns, local)))
            {
                return Ok((value.text().into_owned(), false));
            }
        }
        Ok((String::new(), false))
    }
}

fn marker(run: &mut LensMut<'_, Run>, kind: &str, dirty: bool) -> Result<()> {
    let mut field_char = run.insert::<FieldChar>()?;
    field_char.set_attr(FieldChar::ATTR_W_FLD_CHAR_TYPE, kind)?;
    if dirty {
        field_char.set_attr(FieldChar::ATTR_W_DIRTY, "true")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoryIndex;

    #[test]
    fn scanner_classifies_all_inserted_fields() {
        let source = crate::test_package::docx(
            "<w:p><w:bookmarkStart w:id='1' w:name='Target'/><w:r><w:t>Text</w:t></w:r><w:bookmarkEnd w:id='1'/></w:p><w:p/>",
            None,
        );
        let mut doc = Document::from_bytes(source).unwrap();
        doc.enable_update_fields_on_open().unwrap();
        let at = RunPosition::EndOf(doc.paragraphs().nth(1).unwrap().para_ref());
        let fields = [
            (FieldSpec::Page, "PAGE", " PAGE "),
            (FieldSpec::NumPages, "NUMPAGES", " NUMPAGES "),
            (FieldSpec::SectionPages, "SECTIONPAGES", " SECTIONPAGES "),
            (
                FieldSpec::Date {
                    format: "yyyy-MM-dd".into(),
                },
                "DATE",
                r#" DATE \@ "yyyy-MM-dd" "#,
            ),
            (
                FieldSpec::Time {
                    format: "HH:mm".into(),
                },
                "TIME",
                r#" TIME \@ "HH:mm" "#,
            ),
            (
                FieldSpec::DocProperty {
                    name: "Title".into(),
                },
                "DOCPROPERTY TITLE",
                r#" DOCPROPERTY "Title" "#,
            ),
            (
                FieldSpec::Ref {
                    bookmark: "Target".into(),
                    hyperlink: true,
                },
                "REF",
                r#" REF "Target" \h "#,
            ),
            (
                FieldSpec::PageRef {
                    bookmark: "Target".into(),
                    hyperlink: false,
                },
                "PAGEREF",
                r#" PAGEREF "Target" "#,
            ),
            (
                FieldSpec::Seq {
                    identifier: "Figure".into(),
                },
                "SEQ",
                r#" SEQ "Figure" "#,
            ),
            (
                FieldSpec::Toc {
                    levels: (1, 3),
                    hyperlinks: true,
                },
                "TOC",
                r#" TOC \o "1-3" \h "#,
            ),
        ];
        for (spec, _, _) in &fields {
            doc.insert_field(at, spec.clone()).unwrap();
        }
        let doc = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
        assert_eq!(
            FieldScan::scan(doc.story_root_node(StoryIndex::MAIN)),
            fields.iter().map(|(_, key, _)| *key).collect::<Vec<_>>(),
        );
        let paragraph = doc.paragraphs().nth(1).unwrap();
        let runs: Vec<_> = paragraph.raw().elements().collect();
        assert_eq!(runs.len(), fields.len() * 5);
        for (index, (runs, (_, _, instruction))) in
            runs.as_chunks::<5>().0.iter().zip(fields).enumerate()
        {
            let children: Vec<_> = runs.iter().map(|run| run.elements().next()).collect();
            for (pos, kind) in [(0, "begin"), (2, "separate"), (4, "end")] {
                let fld = children[pos].unwrap();
                assert!(fld.is(w("fldChar")));
                assert_eq!(fld.attr(w("fldCharType")), Some(kind));
                assert_eq!(
                    fld.attr(w("dirty")),
                    (index >= 6 && pos == 0).then_some("true")
                );
            }
            let code = children[1].unwrap();
            assert!(code.is(w("instrText")));
            assert_eq!(code.attr(XML_SPACE), Some("preserve"));
            assert_eq!(code.text(), instruction);
        }
        assert_eq!(
            doc.write_gate().unwrap_err().to_string(),
            Error::StaleFieldRisk {
                codes: ["PAGEREF", "REF", "SEQ", "TOC"].map(str::to_owned).to_vec(),
            }
            .to_string()
        );
    }

    #[test]
    #[ignore = "requires the upstream SDK Word corpus"]
    fn sdk_field_insertion_sweep() {
        let path = std::env::var_os("OOXML_SDK_DOCX_CORPUS").expect("set OOXML_SDK_DOCX_CORPUS");
        let mut files: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                matches!(
                    path.extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref(),
                    Some("docx" | "docm" | "dotx" | "dotm")
                )
            })
            .collect();
        files.sort();
        assert!(!files.is_empty());
        let mut open_refusals = 0;
        let mut no_paragraph = 0;
        let mut successes = [0; 2];
        let mut refusals = std::collections::BTreeMap::<_, usize>::new();
        for path in &files {
            let input = std::fs::read(path).unwrap();
            let Ok(mut doc) = Document::from_bytes(input) else {
                open_refusals += 1;
                continue;
            };
            let Some(at) = doc.paragraphs().next().map(|p| p.para_ref()) else {
                no_paragraph += 1;
                continue;
            };
            let scan = |doc: &Document| {
                let mut codes = FieldScan::scan(doc.story_root_node(StoryIndex::MAIN));
                codes.sort();
                codes
            };
            let mut before = doc.to_bytes().unwrap();
            let mut expected = scan(&doc);
            for (i, (spec, key)) in [
                (FieldSpec::Page, "PAGE"),
                (
                    FieldSpec::Date {
                        format: "yyyy-MM-dd".into(),
                    },
                    "DATE",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let result = doc.insert_field(RunPosition::EndOf(at), spec);
                let after = doc
                    .to_bytes()
                    .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                match result {
                    Ok(()) => {
                        expected.push(key.into());
                        expected.sort();
                        let reopened = Document::from_bytes(after.clone())
                            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                        assert_eq!(scan(&reopened), expected, "{}", path.display());
                        before = after;
                        successes[i] += 1;
                    }
                    Err(err) => {
                        assert!(after == before, "{}: {err}", path.display());
                        *refusals.entry((key, err.to_string())).or_default() += 1;
                    }
                }
            }
        }
        println!(
            "{} packages; {open_refusals} refused to open; {no_paragraph} without a paragraph; PAGE/DATE successes {successes:?}",
            files.len()
        );
        for ((key, error), count) in refusals {
            println!("{count:4} {key}: {error}");
        }
        assert!(successes.into_iter().all(|n| n > 0));
    }
}
