//! Modern PowerPoint comment creation (ADR-0019 Delivery 1).

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use ooxml_core::value::is_xml_char;
use ooxml_core::{ElementSpec, NsDecl};

use super::*;
use crate::read::{self, ShapeKind};

const AUTHORS_REL: &str = "http://schemas.microsoft.com/office/2018/10/relationships/authors";
const AUTHORS_CT: &str = "application/vnd.ms-powerpoint.authors+xml";
pub(super) const COMMENTS_CT: &str = "application/vnd.ms-powerpoint.comments+xml";
const COMMENT_EXT: &str = "{6950BFC3-D8DA-4A85-94F7-54DA5524770B}";
const CREATION_EXT: &str = "{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}";
const SHAPE_CREATION_EXT: &str = "{FF2B5EF4-FFF2-40B4-BE49-F238E27FC236}";
const A16_NS: &str = "http://schemas.microsoft.com/office/drawing/2014/main";
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";

/// Where a modern comment is attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentAnchor {
    Slide,
    Shape(u32),
}

/// The identity used to find a stored comment author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorIdentity {
    Local,
    Account {
        user_id: String,
        provider_id: String,
    },
}

/// Author information used when creating a comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentAuthor {
    pub name: String,
    pub identity: AuthorIdentity,
    pub initials: Option<String>,
}

/// A new thread's author, text, optional creation time and language.
#[derive(Clone, Debug)]
pub struct NewComment {
    pub author: CommentAuthor,
    pub text: String,
    pub created: Option<SystemTime>,
    pub lang: Option<String>,
}

/// A braced, uppercase PowerPoint comment GUID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommentId(String);

impl CommentId {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.strip_circumfix('{', '}').unwrap_or(value);
        (value.len() == 36
            && value.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            }))
        .then(|| Self(format!("{{{}}}", value.to_ascii_uppercase())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CommentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommentPart {
    Authors,
    Threads,
}

impl CommentPart {
    fn from_content_type(content_type: Option<&str>) -> Option<Self> {
        match content_type {
            Some(AUTHORS_CT) => Some(Self::Authors),
            Some(COMMENTS_CT) => Some(Self::Threads),
            _ => None,
        }
    }

    fn error(self, part: &str, reason: impl Into<String>) -> Error {
        let part = part.to_owned();
        let reason = reason.into();
        match self {
            Self::Authors => Error::AuthorsPartNotEditable { part, reason },
            Self::Threads => Error::CommentPartNotEditable { part, reason },
        }
    }

    fn list(self) -> &'static str {
        match self {
            Self::Authors => "authorLst",
            Self::Threads => "cmLst",
        }
    }

    fn child(self) -> &'static str {
        match self {
            Self::Authors => "author",
            Self::Threads => "cm",
        }
    }

    fn read(self, pkg: &mut ooxml_core::Package, part: &str) -> Result<XmlDocument> {
        pkg.xml_part(part)
            .cloned()
            .map_err(|e| self.error(part, e.to_string()))
    }

    fn editable(
        self,
        pkg: &mut ooxml_core::Package,
        part: &str,
        source: &str,
    ) -> Result<XmlDocument> {
        if Self::from_content_type(pkg.content_type(part)) != Some(self) {
            return Err(self.error(part, "unexpected content type"));
        }
        let incoming = pkg.incoming_relationships(part)?;
        if !matches!(&incoming[..], [edge] if edge.source.as_deref() == Some(source)) {
            return Err(self.error(part, "ambiguous part ownership"));
        }
        let doc = self.read(pkg, part)?;
        let root = doc.root();
        if !root.is(q(P188_NS, self.list()))
            || root
                .elements()
                .any(|node| !node.is(q(P188_NS, self.child())))
        {
            return Err(self.error(part, "unexpected list root or child"));
        }
        let mut alternate_content = false;
        walk(root, &mut |node| {
            alternate_content |= node.is(q(MC_NS, "AlternateContent"))
        });
        if alternate_content {
            return Err(self.error(part, "AlternateContent in list"));
        }
        Ok(doc)
    }
}

fn child(node: &RawNode, name: QName) -> Option<&RawNode> {
    node.elements().find(|element| element.is(name))
}

fn attr<'a>(node: &'a RawNode, name: &'static str) -> Option<&'a str> {
    node.attr(q("", name))
}

fn extension<'a>(
    node: &'a RawNode,
    ns: &'static str,
    uri: &'a str,
) -> impl Iterator<Item = &'a RawNode> {
    node.elements()
        .filter(move |element| element.is(q(ns, "extLst")))
        .flat_map(RawNode::elements)
        .filter(move |element| element.is(q(ns, "ext")) && attr(element, "uri") == Some(uri))
}

fn slide_creation_id(root: &RawNode) -> Result<Option<u32>> {
    let unsupported = || Error::UnsupportedContent("invalid slide p14:creationId extension".into());
    let candidates: Vec<_> = root
        .elements()
        .filter(|n| n.is(q(P_NS, "cSld")))
        .flat_map(|data| extension(data, P_NS, CREATION_EXT))
        .collect();
    let mut count = 0;
    walk(root, &mut |node| {
        count += usize::from(node.is(pml::CreationId::QNAME))
    });
    match (candidates.as_slice(), count) {
        ([], 0) => Ok(None),
        ([ext], 1) => child(ext, pml::CreationId::QNAME)
            .and_then(|node| attr(node, "val"))
            .filter(|value| value.trim().bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.trim().parse().ok())
            .map(Some)
            .ok_or_else(unsupported),
        _ => Err(unsupported()),
    }
}

fn insert_creation_id(doc: &mut XmlDocument, id: u32) -> Result<()> {
    let mut slide = doc.lens_mut::<pml::Slide>()?;
    let mut data = slide
        .child_mut::<pml::CommonSlideData>()
        .ok_or_else(|| Error::UnsupportedContent("missing p:cSld".into()))?;
    let mut list = if child(data.node(), pml::CommonSlideDataExtensionList::QNAME).is_some() {
        data.get_or_insert::<pml::CommonSlideDataExtensionList>()?
    } else {
        data.insert_after_alternate_content::<pml::CommonSlideDataExtensionList>()?
    };
    let mut ext = list.insert::<pml::CommonSlideDataExtension>()?;
    ext.set_attr(q("", "uri"), CREATION_EXT)?;
    let mut creation = ext.insert_with_ns::<pml::CreationId>(&[NsDecl::new("p14", P14_NS)])?;
    ooxml_core::value::with_u32(id, |value| creation.set_attr(pml::CreationId::ATTR_VAL, value))?;
    Ok(())
}

fn anchor_xml(
    doc: &XmlDocument,
    anchor: CommentAnchor,
    slide_id: u32,
    creation_id: u32,
) -> Result<String> {
    let CommentAnchor::Shape(id) = anchor else {
        return Ok(format!(
            "<pc:sldMkLst xmlns:pc=\"{PC_NS}\"><pc:docMk/><pc:sldMk cId=\"{creation_id}\" sldId=\"{slide_id}\"/></pc:sldMkLst>"
        ));
    };
    let slide = format!(
        "<pc:docMk xmlns:pc=\"{PC_NS}\"/><pc:sldMk xmlns:pc=\"{PC_NS}\" cId=\"{creation_id}\" sldId=\"{slide_id}\"/>"
    );
    let shape = read::find_by_id(read::shapes_of(doc), id)?.ok_or(Error::ShapeNotFound)?;
    let kind = match shape.kind {
        ShapeKind::Shape => "spMk",
        ShapeKind::Group => "grpSpMk",
        ShapeKind::Table | ShapeKind::GraphicFrame { .. } => "graphicFrameMk",
        ShapeKind::Picture => "picMk",
        ShapeKind::Connector => "cxnSpMk",
        ShapeKind::AlternateContent => return Err(Error::InsideAlternateContent),
        _ => {
            return Err(Error::UnsupportedCommentAnchor(
                shape.node.name().local().into(),
            ));
        }
    };
    let path = shape.path();
    for depth in 3..=path.0.len() {
        let node =
            crate::write::node_at(doc.root(), &path.0[..depth]).ok_or(Error::InvalidShapeId)?;
        read::raw_shape_id(node).ok_or(Error::InvalidShapeId)?;
    }
    let creation = read::c_nv_pr(shape.node)
        .into_iter()
        .flat_map(|properties| extension(properties, A_NS, SHAPE_CREATION_EXT))
        .find_map(|node| child(node, q(A16_NS, "creationId")))
        .and_then(|node| attr(node, "id"))
        .map_or_default(|value| format!(" creationId=\"{}\"", escape_attr(value)));
    Ok(format!(
        "<ac:deMkLst xmlns:ac=\"{AC_NS}\">{slide}<ac:{kind} id=\"{id}\"{creation}/></ac:deMkLst>"
    ))
}

fn escape_attr(value: &str) -> String {
    xml_escape(value)
        .replace('\t', "&#9;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;")
}

fn root_declarations(
    kind: CommentPart,
    part: &str,
    root: &RawNode,
    prefixes: &[(&'static str, &str)],
) -> Result<String> {
    let mut result = String::new();
    for &(prefix, uri) in prefixes {
        match root.attr(q(XMLNS, prefix)) {
            Some(bound) if bound == uri => {}
            Some(_) => {
                return Err(kind.error(part, format!("conflicting namespace prefix {prefix}")));
            }
            None => write!(result, " xmlns:{prefix}=\"{uri}\"").unwrap(),
        }
    }
    Ok(result)
}

/// Inserts a fixed-form element at the end of a validated list, preserving
/// its original spelling, including any aliases on the existing root.
fn append_xml(
    doc: &XmlDocument,
    fragment: &str,
    options: &ooxml_core::ParseOptions,
) -> Result<XmlDocument> {
    let xml = doc.serialize();
    let parsed = XmlDocument::parse_with(xml.as_bytes(), options)?;
    let root = parsed.root();
    let span = root.span().expect("parsed root has a span");
    let mut output = xml;
    if let Some((close, _)) = span.close {
        output.insert_str(close, fragment);
    } else {
        let name = root.name();
        let close = name.prefix().map_or_else(
            || name.local().to_owned(),
            |p| format!("{p}:{}", name.local()),
        );
        output.replace_range(
            span.open_end - 2..span.open_end,
            &format!(">{fragment}</{close}>"),
        );
    }
    Ok(XmlDocument::parse_with(output.as_bytes(), options)?)
}

fn new_list(
    kind: CommentPart,
    fragment: &str,
    options: &ooxml_core::ParseOptions,
) -> Result<XmlDocument> {
    let list = kind.list();
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p188:{list} xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p188=\"{P188_NS}\">{fragment}</p188:{list}>"
    );
    Ok(XmlDocument::parse_with(xml.as_bytes(), options)?)
}

fn target(
    pkg: &ooxml_core::Package,
    source: &str,
    rel_type: &str,
    kind: CommentPart,
) -> Result<Option<String>> {
    let rels = pkg.relationships(source)?;
    let mut matches = rels.by_type(rel_type);
    let Some(rel) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(kind.error(source, "multiple relationships"));
    }
    rels.resolve(rel)
        .map(Some)
        .ok_or_else(|| kind.error(source, "invalid relationship target"))
}

fn normalized_id(value: &str) -> String {
    CommentId::parse(value).map_or_else(|| value.to_ascii_uppercase(), |id| id.0)
}

fn scan_ids(pkg: &mut ooxml_core::Package, skip_unreadable: bool) -> Result<HashSet<String>> {
    let parts: Vec<_> = pkg
        .part_names()
        .filter_map(|part| {
            Some((
                part.to_owned(),
                CommentPart::from_content_type(pkg.content_type(part))?,
            ))
        })
        .collect();
    let mut ids = HashSet::new();
    for (part, kind) in parts {
        let doc = match pkg.xml_part(&part) {
            Ok(doc) => doc,
            Err(_) if skip_unreadable => continue,
            Err(_) => return Err(kind.error(&part, format!("id scan: {part} does not parse"))),
        };
        walk(doc.root(), &mut |node| {
            if ["cm", "reply", "author"]
                .iter()
                .any(|name| node.is(q(P188_NS, name)))
                && let Some(id) = attr(node, "id")
            {
                ids.insert(normalized_id(id));
            }
        });
    }
    Ok(ids)
}

pub(super) fn reserve_ids_before_delete(pkg: &mut ooxml_core::Package) -> HashSet<String> {
    scan_ids(pkg, true).expect("unreadable parts are skipped")
}

fn allocate_id(
    ids: &mut HashSet<String>,
    random: &mut impl FnMut(&mut [u8; 16]) -> Result<()>,
) -> Result<CommentId> {
    for _ in 0..32 {
        let mut bytes = [0; 16];
        random(&mut bytes)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let mut id = String::from("{");
        for (index, byte) in bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                id.push('-');
            }
            write!(id, "{byte:02X}").unwrap();
        }
        id.push('}');
        if ids.insert(id.clone()) {
            return Ok(CommentId(id));
        }
    }
    Err(Error::Random("32 colliding draws".into()))
}

fn timestamp(time: SystemTime) -> Result<String> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidCommentTime)?;
    if duration.as_secs() >= 253_402_300_800 {
        return Err(Error::InvalidCommentTime);
    }
    let utc = crate::new_deck::w3cdtf(time);
    Ok(format!(
        "{}.{:03}",
        &utc[..utc.len() - 1],
        duration.subsec_millis()
    ))
}

fn body(text: &str, lang: &str) -> Result<String> {
    if text.is_empty() {
        return Err(Error::EmptyCommentText);
    }
    let mut subtags = lang.split('-');
    let primary = subtags.next().expect("split always has a first item");
    if !(2..=8).contains(&primary.len())
        || !primary.bytes().all(|b| b.is_ascii_alphabetic())
        || subtags.any(|tag| {
            !(1..=8).contains(&tag.len()) || !tag.bytes().all(|b| b.is_ascii_alphanumeric())
        })
    {
        return Err(Error::InvalidCommentLang);
    }
    let mut xml = String::from("<p188:txBody><a:bodyPr/><a:lstStyle/>");
    for lines in crate::text::body_lines(text)? {
        xml.push_str("<a:p>");
        if lines.len() == 1 && lines[0].is_empty() {
            write!(xml, "<a:endParaRPr lang=\"{lang}\"/>").unwrap();
        } else {
            for (index, line) in lines.iter().enumerate() {
                if index != 0 {
                    xml.push_str("<a:br/>");
                }
                if !line.is_empty() {
                    write!(
                        xml,
                        "<a:r><a:rPr lang=\"{lang}\"/><a:t>{}</a:t></a:r>",
                        xml_escape(line).replace('>', "&gt;")
                    )
                    .unwrap();
                }
            }
        }
        xml.push_str("</a:p>");
    }
    xml.push_str("</p188:txBody>");
    Ok(xml)
}

impl CommentAuthor {
    fn identity_fields(&self) -> Result<(&str, &str)> {
        let (user, provider) = match &self.identity {
            AuthorIdentity::Local => (self.name.as_str(), "None"),
            AuthorIdentity::Account { provider_id, .. } if provider_id == "None" => {
                return Err(Error::InvalidCommentAuthor {
                    reason: "account provider is None".into(),
                });
            }
            AuthorIdentity::Account {
                user_id,
                provider_id,
            } => (user_id.as_str(), provider_id.as_str()),
        };
        for (field, value) in [
            ("name", self.name.as_str()),
            ("userId", user),
            ("providerId", provider),
        ] {
            if value.is_empty() || !value.chars().all(is_xml_char) {
                return Err(Error::InvalidCommentAuthor {
                    reason: format!("invalid {field}"),
                });
            }
        }
        if self.initials.as_ref().is_some_and(|initials| {
            initials.is_empty()
                || initials
                    .chars()
                    .any(|ch| !is_xml_char(ch) || ch.is_whitespace())
        }) {
            return Err(Error::InvalidCommentAuthor {
                reason: "invalid initials".into(),
            });
        }
        Ok((user, provider))
    }

    fn default_initials(&self) -> String {
        let mut words = self.name.split_whitespace();
        let first = words.next().and_then(|word| word.chars().next());
        let last = words.last().and_then(|word| word.chars().next());
        first
            .into_iter()
            .chain(last)
            .flat_map(char::to_uppercase)
            .collect()
    }
}

fn stage_author(
    pkg: &mut ooxml_core::Package,
    plan: &mut RegistryPlan,
    main: &str,
    author: &CommentAuthor,
    mut next_id: impl FnMut() -> Result<CommentId>,
) -> Result<String> {
    let (user, provider) = author.identity_fields()?;
    let kind = CommentPart::Authors;
    let part = target(pkg, main, AUTHORS_REL, kind)?;
    let doc = part
        .as_deref()
        .map(|part| kind.editable(pkg, part, main))
        .transpose()?;
    let mut declarations = String::new();
    if let Some((part, doc)) = part.as_deref().zip(doc.as_ref()) {
        declarations = root_declarations(kind, part, doc.root(), &[("p188", P188_NS)])?;
        let mut ids = HashSet::new();
        let mut matched = None;
        for entry in doc.root().elements() {
            let id = attr(entry, "id");
            if id.is_some_and(|id| !ids.insert(normalized_id(id))) {
                return Err(kind.error(part, "duplicate author id"));
            }
            let same_identity = attr(entry, "providerId") == Some(provider)
                && if provider == "None" {
                    attr(entry, "name") == Some(&author.name)
                } else {
                    attr(entry, "userId") == Some(user)
                };
            if same_identity && matched.is_none() {
                matched = Some(
                    id.filter(|id| CommentId::parse(id).is_some())
                        .ok_or_else(|| kind.error(part, "matching author has invalid id")),
                );
            }
        }
        if let Some(id) = matched {
            return id.map(str::to_owned);
        }
    }
    let id = next_id()?;
    let initials = author
        .initials
        .clone()
        .unwrap_or_else(|| author.default_initials());
    let initials = if initials.is_empty() {
        String::new()
    } else {
        format!(" initials=\"{}\"", escape_attr(&initials))
    };
    let fragment = format!(
        "<p188:author{declarations} id=\"{id}\" name=\"{}\"{initials} userId=\"{}\" providerId=\"{}\"/>",
        escape_attr(&author.name),
        escape_attr(user),
        escape_attr(provider)
    );
    if let Some((part, doc)) = part.zip(doc) {
        plan.stage_part(
            pkg,
            &part,
            append_xml(&doc, &fragment, &pkg.parse_options())?,
        )?;
    } else {
        let part = plan.add_part(
            pkg,
            PartNameRequest::Exact("ppt/authors.xml".into()),
            AUTHORS_CT,
            new_list(kind, &fragment, &pkg.parse_options())?,
        )?;
        plan.add_relationship(pkg, Some(main), AUTHORS_REL, Target::Part(part))?;
    }
    Ok(id.0)
}

impl crate::Presentation {
    /// Adds a modern comment to a slide (zero-based index) or a drawing element.
    ///
    /// The comment, author registration and slide changes are committed in one
    /// transaction. A deck containing a legacy comments relationship is refused.
    pub fn add_comment(
        &mut self,
        slide: usize,
        anchor: CommentAnchor,
        comment: NewComment,
    ) -> Result<CommentId> {
        self.add_comment_with_random(slide, anchor, comment, |bytes| {
            getrandom::fill(bytes).map_err(|error| Error::Random(error.to_string()))
        })
    }

    fn add_comment_with_random(
        &mut self,
        index: usize,
        anchor: CommentAnchor,
        comment: NewComment,
        mut random: impl FnMut(&mut [u8; 16]) -> Result<()>,
    ) -> Result<CommentId> {
        let mut slides: BTreeSet<_> = self
            .slides
            .iter()
            .filter_map(|slide| slide.part.as_deref())
            .collect();
        slides.extend(
            self.pkg
                .part_names()
                .filter(|part| self.pkg.content_type(part) == Some(CT_SLIDE)),
        );
        let mut unreadable = None;
        for part in slides {
            match self.pkg.relationships(part) {
                Ok(rels) if rels.by_type(REL_COMMENTS).next().is_some() => {
                    return Err(Error::LegacyCommentsInDeck);
                }
                Err(error) if unreadable.is_none() => unreadable = Some(error),
                _ => {}
            }
        }
        if let Some(error) = unreadable {
            return Err(error.into());
        }
        let source = self.slide_part(index)?;
        let slide_id = self.slides[index].slide_id;
        let body = body(&comment.text, comment.lang.as_deref().unwrap_or("en-US"))?;
        let created = timestamp(comment.created.unwrap_or_else(SystemTime::now))?;
        let mut slide = self.tree(&source)?.clone();
        let kind = CommentPart::Threads;
        let part = target(&self.pkg, &source, MODERN_COMMENTS_REL, kind)?;
        let existing = part
            .as_deref()
            .map(|part| kind.editable(&mut self.pkg, part, &source))
            .transpose()?;
        let (c_id, insert_id) = match slide_creation_id(slide.root())? {
            Some(id) => (id, false),
            None if existing.is_some() => {
                return Err(Error::UnsupportedContent(
                    "missing slide p14:creationId for existing comments part".into(),
                ));
            }
            None => (
                self.max_creation_id()?
                    .checked_add(1)
                    .ok_or(Error::CreationIdsExhausted)?,
                true,
            ),
        };
        let anchor = anchor_xml(&slide, anchor, slide_id, c_id)?;
        let mut ids = scan_ids(&mut self.pkg, false)?;
        ids.extend(self.comment_ids.iter().cloned());
        let mut plan = RegistryPlan::new(&self.pkg);
        let author_id = stage_author(
            &mut self.pkg,
            &mut plan,
            &self.main,
            &comment.author,
            || allocate_id(&mut ids, &mut random),
        )?;
        let id = allocate_id(&mut ids, &mut random)?;
        let declarations = if let Some((part, doc)) = part.as_deref().zip(existing.as_ref()) {
            root_declarations(
                kind,
                part,
                doc.root(),
                &[("p188", P188_NS), ("a", A_NS), ("ac", AC_NS), ("pc", PC_NS)],
            )?
        } else {
            String::new()
        };
        let fragment = format!(
            "<p188:cm{declarations} id=\"{id}\" authorId=\"{author_id}\" created=\"{created}\">{anchor}{body}</p188:cm>"
        );
        if let Some((part, existing)) = part.zip(existing) {
            plan.stage_part(
                &self.pkg,
                &part,
                append_xml(&existing, &fragment, &self.pkg.parse_options())?,
            )?;
        } else {
            if extension(slide.root(), P_NS, COMMENT_EXT).next().is_some() {
                return Err(kind.error(&source, "comment extension without a relationship"));
            }
            if insert_id {
                insert_creation_id(&mut slide, c_id)?;
            }
            let part = plan.add_part(
                &self.pkg,
                PartNameRequest::Exact(format!(
                    "ppt/comments/modernComment_{slide_id:X}_{c_id:X}.xml"
                )),
                COMMENTS_CT,
                new_list(kind, &fragment, &self.pkg.parse_options())?,
            )?;
            let rid = plan.add_relationship(
                &self.pkg,
                Some(&source),
                MODERN_COMMENTS_REL,
                Target::Part(part),
            )?;
            let r_binding = child(slide.root(), pml::SlideExtensionList::QNAME)
                .and_then(|list| list.attr(q(XMLNS, "r")))
                .or_else(|| slide.root().attr(q(XMLNS, "r")));
            let mut declarations = vec![NsDecl::new("p188", P188_NS)];
            if r_binding != Some(R_NS) {
                declarations.push(NsDecl::new("r", R_NS));
            }
            let mut root = slide.lens_mut::<pml::Slide>()?;
            let mut list = if child(root.node(), pml::SlideExtensionList::QNAME).is_some() {
                root.get_or_insert::<pml::SlideExtensionList>()?
            } else {
                root.insert_after_alternate_content::<pml::SlideExtensionList>()?
            };
            let mut ext = list.insert::<pml::SlideExtension>()?;
            ext.set_attr(q("", "uri"), COMMENT_EXT)?;
            let mut rel = ext.insert_with_ns::<pml::CommentRelationship>(&declarations)?;
            rel.set_attr(q(R_NS, "id"), &rid)?;
            plan.stage_part(&self.pkg, &source, slide)?;
        }
        self.finish(plan)?;
        self.comment_ids = ids;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, Presentation};
    use std::{assert_matches, time::Duration};

    fn open(path: &str) -> Presentation {
        Presentation::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../reference/fixtures/pptx")
                .join(path),
        )
        .unwrap()
    }

    fn comment() -> NewComment {
        NewComment {
            author: CommentAuthor {
                name: "Ada Lovelace".into(),
                identity: AuthorIdentity::Local,
                initials: None,
            },
            text: "A & B\r\n\rC\u{b}D\t>".into(),
            created: Some(UNIX_EPOCH + Duration::from_millis(951_782_400_123)),
            lang: None,
        }
    }

    fn refuses(
        deck: &mut Presentation,
        expected: impl FnOnce(Error),
        operation: impl FnOnce(&mut Presentation) -> Result<CommentId>,
    ) {
        let before = deck.to_bytes().unwrap();
        expected(operation(deck).unwrap_err());
        assert_eq!(deck.to_bytes().unwrap(), before);
    }

    #[test]
    fn new_comment_round_trip_and_locality() {
        let mut deck = open("poi/sample.pptx");
        let before = deck.to_bytes().unwrap();
        let mut draw = 0;
        let id = deck
            .add_comment_with_random(0, CommentAnchor::Slide, comment(), |bytes| {
                draw += 1;
                bytes.fill(draw);
                Ok(())
            })
            .unwrap();
        assert_eq!(id.as_str(), "{02020202-0202-4202-8202-020202020202}");
        let mut output = Presentation::from_bytes(deck.to_bytes().unwrap()).unwrap();
        let view = output.deck().unwrap();
        let slide = view.slide(0).unwrap();
        let [entry] = slide.comments().unwrap().try_into().unwrap();
        assert_eq!(entry.id, Some(id.as_str()));
        assert_eq!(entry.anchor, Anchor::Slide);
        assert_eq!(entry.created, Some("2000-02-29T00:00:00.123"));
        assert_eq!(entry.text, "A & B\n\nC\u{b}D\t>");
        assert_eq!(entry.author.unwrap().initials, Some("AL"));
        let input = Presentation::from_bytes(before).unwrap();
        let source = input.slides[0].part.as_deref().unwrap();
        let rels = ooxml_core::opc::rels_part_name(Some(source));
        let main_rels = ooxml_core::opc::rels_part_name(Some(&input.main));
        for part in input.pkg.part_names() {
            if [
                source,
                rels.as_str(),
                main_rels.as_str(),
                "[Content_Types].xml",
            ]
            .contains(&part)
            {
                continue;
            }
            assert_eq!(
                input.pkg.read_part(part).unwrap(),
                output.pkg.read_part(part).unwrap(),
                "{part}"
            );
        }
    }

    #[test]
    fn append_flat_monikers_and_keep_existing_author() {
        let mut deck = open("project/powerpoint-comment-anchor-kinds.pptx");
        let mut checked = 0;
        for index in 0..deck.slide_count() {
            let source = deck.slides[index].part.clone().unwrap();
            let xml = deck.pkg.read_part(&source).unwrap();
            let shapes: Vec<_> = deck
                .slide(index)
                .unwrap()
                .shapes()
                .into_iter()
                .map(|shape| shape.id().unwrap())
                .collect();
            for id in shapes {
                let guid = deck
                    .add_comment(index, CommentAnchor::Shape(id), comment())
                    .unwrap();
                let view = deck.deck().unwrap();
                let slide = view.slide(index).unwrap();
                let comments = slide.comments().unwrap();
                let new = comments.last().unwrap();
                assert_eq!(new.id, Some(guid.as_str()));
                assert_eq!(new.anchor, Anchor::Shapes(vec![id]));
                checked += 1;
            }
            assert_eq!(deck.pkg.read_part(&source).unwrap(), xml);
        }
        assert!(checked >= 5);

        let mut local = open("project/powerpoint-shape-comment.pptx");
        let xml = local.pkg.read_part("ppt/authors.xml").unwrap();
        local
            .add_comment(0, CommentAnchor::Shape(2), comment())
            .unwrap();
        assert_eq!(local.pkg.read_part("ppt/authors.xml").unwrap(), xml);
    }

    #[test]
    fn comment_extension_follows_alternate_content() {
        let mut deck = open("poi/bug68703.pptx");
        let source = deck.slides[0].part.clone().unwrap();
        let before = deck.tree(&source).unwrap();
        let alternate = before
            .root()
            .elements()
            .find(|node| node.is(q(MC_NS, "AlternateContent")))
            .unwrap();
        let span = alternate.span().unwrap();
        let original = before.source()[span.start..span.end()].to_owned();
        deck.add_comment(0, CommentAnchor::Slide, comment())
            .unwrap();
        let after = deck.tree(&source).unwrap().serialize();
        assert!(after.contains(&original));
        assert!(after.find(&original).unwrap() < after.find(COMMENT_EXT).unwrap());
    }

    #[test]
    fn account_identity_uses_provider_and_user() {
        let mut deck = open("poi/sample.pptx");
        let mut c = comment();
        c.author.identity = AuthorIdentity::Account {
            user_id: "user".into(),
            provider_id: "AD".into(),
        };
        deck.add_comment(0, CommentAnchor::Slide, c.clone())
            .unwrap();
        let first = deck.pkg.read_part("ppt/authors.xml").unwrap();
        c.author.name = "Renamed Display Name".into();
        deck.add_comment(0, CommentAnchor::Slide, c.clone())
            .unwrap();
        assert_eq!(deck.pkg.read_part("ppt/authors.xml").unwrap(), first);
        c.author.identity = AuthorIdentity::Account {
            user_id: "user".into(),
            provider_id: "Other".into(),
        };
        deck.add_comment(0, CommentAnchor::Slide, c.clone())
            .unwrap();
        let view = deck.deck().unwrap();
        let comments = view.slide(0).unwrap().comments().unwrap();
        let authors: Vec<_> = comments
            .iter()
            .map(|c| c.author.as_ref().unwrap())
            .collect();
        assert_eq!(authors[0], authors[1]);
        assert_ne!(authors[1].id, authors[2].id);
        c.author.identity = AuthorIdentity::Account {
            user_id: "user".into(),
            provider_id: "None".into(),
        };
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::InvalidCommentAuthor { .. }),
            |d| d.add_comment(0, CommentAnchor::Slide, c),
        );
    }

    #[test]
    fn validation_and_random_refuse_atomically() {
        let mut legacy = open("poi/45545_Comment.pptx");
        refuses(
            &mut legacy,
            |e| assert_matches!(e, Error::LegacyCommentsInDeck),
            |d| d.add_comment(2, CommentAnchor::Slide, comment()),
        );
        let mut deck = open("poi/sample.pptx");
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::ShapeNotFound),
            |d| d.add_comment(0, CommentAnchor::Shape(u32::MAX), comment()),
        );
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::Random(_)),
            |d| d.add_comment_with_random(0, CommentAnchor::Slide, comment(), |_| Ok(())),
        );
        let invalid: [(NewComment, fn(Error)); 5] = [
            (
                NewComment {
                    text: "".into(),
                    ..comment()
                },
                |e| assert_matches!(e, Error::EmptyCommentText),
            ),
            (
                NewComment {
                    text: "\u{1}".into(),
                    ..comment()
                },
                |e| assert_matches!(e, Error::InvalidXmlChar { .. }),
            ),
            (
                NewComment {
                    lang: Some("en--US".into()),
                    ..comment()
                },
                |e| assert_matches!(e, Error::InvalidCommentLang),
            ),
            (
                NewComment {
                    created: Some(UNIX_EPOCH - Duration::from_secs(1)),
                    ..comment()
                },
                |e| assert_matches!(e, Error::InvalidCommentTime),
            ),
            (
                NewComment {
                    author: CommentAuthor {
                        initials: Some("A L".into()),
                        ..comment().author
                    },
                    ..comment()
                },
                |e| assert_matches!(e, Error::InvalidCommentAuthor { .. }),
            ),
        ];
        for (comment, check) in invalid {
            refuses(&mut deck, check, |d| {
                d.add_comment(0, CommentAnchor::Slide, comment)
            });
        }
    }

    #[test]
    fn structural_refusals_are_atomic() {
        let mut deck = open("project/powerpoint-shape-comment.pptx");
        let source = deck.slides[0].part.clone().unwrap();
        let part = target(
            &deck.pkg,
            &source,
            MODERN_COMMENTS_REL,
            CommentPart::Threads,
        )
        .unwrap()
        .unwrap();
        deck.pkg.set_content_type(&part, "application/xml").unwrap();
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::CommentPartNotEditable { reason, .. } if reason == "unexpected content type"),
            |d| d.add_comment(0, CommentAnchor::Slide, comment()),
        );
        deck.pkg.set_content_type(&part, COMMENTS_CT).unwrap();
        let xml = String::from_utf8(deck.pkg.read_part(&part).unwrap()).unwrap();
        for fragment in [
            "<p188:extLst/>",
            "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"/>",
        ] {
            let patched = xml.replace("</p188:cmLst>", &format!("{fragment}</p188:cmLst>"));
            deck.pkg.set_part(&part, patched.into_bytes()).unwrap();
            deck = Presentation::from_bytes(deck.to_bytes().unwrap()).unwrap();
            refuses(
                &mut deck,
                |e| assert_matches!(e, Error::CommentPartNotEditable { .. }),
                |d| d.add_comment(0, CommentAnchor::Slide, comment()),
            );
        }
        deck.pkg.set_part(&part, xml.into_bytes()).unwrap();
        let authors = String::from_utf8(deck.pkg.read_part("ppt/authors.xml").unwrap()).unwrap();
        let duplicate = &authors
            [authors.find("<p188:author ").unwrap()..authors.find("</p188:authorLst>").unwrap()];
        deck.pkg
            .set_part(
                "ppt/authors.xml",
                authors
                    .replace(
                        "</p188:authorLst>",
                        &format!("{duplicate}</p188:authorLst>"),
                    )
                    .into_bytes(),
            )
            .unwrap();
        deck = Presentation::from_bytes(deck.to_bytes().unwrap()).unwrap();
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::AuthorsPartNotEditable { .. }),
            |d| d.add_comment(0, CommentAnchor::Slide, comment()),
        );
        deck.pkg
            .set_part("ppt/authors.xml", authors.into_bytes())
            .unwrap();
        let slide = String::from_utf8(deck.pkg.read_part(&source).unwrap()).unwrap();
        let invalid = slide.replace("val=\"3181846367\"", "val=\"not-a-u32\"");
        assert_ne!(slide, invalid);
        deck.pkg.set_part(&source, invalid.into_bytes()).unwrap();
        deck = Presentation::from_bytes(deck.to_bytes().unwrap()).unwrap();
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::UnsupportedContent(_)),
            |d| d.add_comment(0, CommentAnchor::Slide, comment()),
        );
    }

    #[test]
    fn unrelated_ids_and_unreadable_parts() {
        // SYNTHETIC: an unreferenced comment part still owns its GUID.
        let mut deck = open("poi/sample.pptx");
        let mut plan = RegistryPlan::new(&deck.pkg);
        let fragment = "<p188:cm id=\"{00000000-0000-4000-8000-000000000000}\"/>";
        let part = plan
            .add_part(
                &deck.pkg,
                PartNameRequest::Exact("ppt/comments/orphan.xml".into()),
                COMMENTS_CT,
                new_list(CommentPart::Threads, fragment, &deck.pkg.parse_options()).unwrap(),
            )
            .unwrap();
        deck.pkg.apply(plan).unwrap();
        let mut draw = 0;
        let id = deck
            .add_comment_with_random(0, CommentAnchor::Slide, comment(), |bytes| {
                bytes.fill(draw);
                draw += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(id.as_str(), "{02020202-0202-4202-8202-020202020202}");
        deck.pkg.set_part(&part, b"<".to_vec()).unwrap();
        refuses(
            &mut deck,
            |e| assert_matches!(e, Error::CommentPartNotEditable { reason, .. } if reason.starts_with("id scan:")),
            |d| d.add_comment(0, CommentAnchor::Slide, comment()),
        );
    }
}
