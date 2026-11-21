use std::collections::HashSet;
use std::time::SystemTime;

use ooxml_core::{
    ElementSpec, Package, PartNameRequest, QName, RawNode, RegistryPlan, XmlDocument,
};
use ooxml_schema::sml;

use super::xml::{self, attr, child};
use crate::images::part_error;
use crate::{CellIndex, Error, Result, Workbook, escape};

const NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments";
const EXT: QName = QName::new(NS, "extLst");
const TEXT: QName = QName::new(NS, "text");
const PERSONS: QName = sml::PersonList::QNAME;
const PERSON: QName = sml::Person::QNAME;
const PERSON_TYPE: &str = "application/vnd.ms-excel.person+xml";
pub(super) const PERSON_REL: &str =
    "http://schemas.microsoft.com/office/2017/10/relationships/person";
pub(super) const ROOT: QName = sml::ThreadedComments::QNAME;
pub(super) const ENTRY: QName = sml::ThreadedComment::QNAME;
pub(super) const TYPE: &str = "application/vnd.ms-excel.threadedcomments+xml";
pub(super) const REL: &str =
    "http://schemas.microsoft.com/office/2017/10/relationships/threadedComment";
pub(super) const FALLBACK: &str = "[Threaded comment]\n\n";

/// Identity used to find or create a comment author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonId {
    /// A local author with the display name as user ID and `None` as provider.
    None,
    /// An author identified by a user ID at a provider.
    External {
        /// Provider's user identifier.
        user_id: String,
        /// Identity provider.
        provider_id: String,
    },
}

/// An author in the workbook's threaded-comment person list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person(String);

impl Person {
    /// The author's Open XML identifier.
    pub fn id(&self) -> &str {
        &self.0
    }
}

/// Options for a threaded comment or reply.
#[derive(Clone, Debug)]
pub struct Comment {
    pub(super) text: String,
    person: Option<Person>,
    timestamp: Option<SystemTime>,
}

impl Comment {
    /// Creates a comment. Use [`Self::by`] to supply its author.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            person: None,
            timestamp: None,
        }
    }

    /// Sets the author returned by [`Workbook::person`].
    pub fn by(mut self, person: impl std::borrow::Borrow<Person>) -> Self {
        self.person = Some(person.borrow().clone());
        self
    }

    /// Sets the instant written as a zone-less UTC timestamp.
    pub fn timestamp(mut self, timestamp: SystemTime) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    pub(super) fn markup(
        &self,
        wb: &Workbook,
        at: CellIndex,
        id: &str,
        parent: Option<&str>,
        root: Option<&RawNode>,
    ) -> Result<String> {
        let invalid_author = || Error::InvalidSetting {
            what: "comment author".into(),
            reason: "expected a person in this workbook".into(),
        };
        let person = self.person.as_ref().ok_or_else(invalid_author)?;
        let (_, people) = person_part(wb)?.ok_or_else(invalid_author)?;
        if !people
            .root()
            .elements()
            .any(|node| node.is(PERSON) && node.attr(attr("id")) == Some(person.id()))
        {
            return Err(invalid_author());
        }
        let timestamp = crate::core_props::w3cdtf(self.timestamp.unwrap_or_else(SystemTime::now));
        let timestamp = timestamp.strip_suffix('Z').expect("UTC formatter");
        let person = xml::attribute(person.id());
        let parent = parent
            .map(|id| format!(" parentId=\"{}\"", xml::attribute(id)))
            .unwrap_or_default();
        let text = xml::text(&escape::encode(&self.text));
        let ns = root
            .map_or_default(|root| xml::namespace_declaration(xml::default_namespace(root), NS));
        Ok(format!(
            "<threadedComment{ns} ref=\"{at}\" dT=\"{timestamp}\" personId=\"{person}\" id=\"{id}\"{parent}><text xml:space=\"preserve\">{text}</text></threadedComment>"
        ))
    }
}

fn person_part(wb: &Workbook) -> Result<Option<(String, XmlDocument)>> {
    xml::target(&wb.pkg, &wb.workbook_part, PERSON_REL)?
        .map(|part| Ok((part.clone(), xml::read(&wb.pkg, &part, PERSONS)?)))
        .transpose()
}

impl Workbook {
    /// Reuses an exact author identity or adds it to the workbook person list.
    pub fn person(&mut self, display_name: &str, identity: PersonId) -> Result<Person> {
        self.check_editable()?;
        let (user_id, provider_id) = match &identity {
            PersonId::None => (display_name, "None"),
            PersonId::External {
                user_id,
                provider_id,
            } => (user_id.as_str(), provider_id.as_str()),
        };
        let existing = person_part(self)?;
        if let Some((part, doc)) = &existing {
            xml::ordered(doc.root(), PERSONS, &[PERSON, EXT], part)?;
            for node in doc.root().elements().filter(|node| node.is(PERSON)) {
                if node.attr(attr("displayName")) == Some(display_name)
                    && node.attr(attr("userId")) == Some(user_id)
                    && node.attr(attr("providerId")) == Some(provider_id)
                {
                    let id = node
                        .attr(attr("id"))
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| part_error(part, "matching person has no id"))?;
                    return Ok(Person(id.into()));
                }
            }
        }
        let id = guid(self)?;
        let (display, user, provider) = (
            xml::attribute(display_name),
            xml::attribute(user_id),
            xml::attribute(provider_id),
        );
        let ns = existing.as_ref().map_or_default(|(_, doc)| {
            xml::namespace_declaration(xml::default_namespace(doc.root()), NS)
        });
        let markup = format!(
            "<person{ns} displayName=\"{display}\" id=\"{id}\" userId=\"{user}\" providerId=\"{provider}\"/>"
        );
        let pkg = &self.pkg;
        let mut plan = RegistryPlan::new(pkg);
        if let Some((part, doc)) = existing {
            xml::require_owner(pkg, &part, &self.workbook_part)?;
            let root = doc.root();
            let edits = vec![xml::insert(root, child(root, EXT), markup)];
            xml::stage(pkg, &mut plan, &part, &doc, edits)?;
        } else {
            xml::add(
                pkg,
                &mut plan,
                &self.workbook_part,
                PERSON_REL,
                PartNameRequest::Exact("xl/persons/person.xml".into()),
                PERSON_TYPE,
                format!("<personList xmlns=\"{NS}\">{markup}</personList>"),
            )?;
        }
        self.apply_registry_edit(plan, |wb| {
            wb.fault_point(crate::workbook::ApplyStep::CommentIndex)?;
            wb.comment_guids.insert(id.clone());
            Ok(())
        })?;
        Ok(Person(id))
    }
}

fn known_ids(pkg: &Package) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for part in pkg.part_names() {
        let (root, entry) = match pkg.content_type(part) {
            Some(PERSON_TYPE) => (PERSONS, PERSON),
            Some(TYPE) => (ROOT, ENTRY),
            _ => continue,
        };
        let doc = xml::read(pkg, part, root)?;
        ids.extend(
            crate::images::walk(doc.root())
                .filter(|node| node.is(entry))
                .filter_map(|node| node.attr(attr("id")))
                .map(str::to_ascii_uppercase),
        );
    }
    Ok(ids)
}

fn random_bytes(_wb: &Workbook) -> Result<[u8; 16]> {
    #[cfg(test)]
    if let Some(source) = _wb.comment_random {
        return Ok(source());
    }
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| Error::Random(error.to_string()))?;
    Ok(bytes)
}

pub(super) fn guid(wb: &mut Workbook) -> Result<String> {
    if !wb.comment_guids_scanned {
        wb.comment_guids.extend(known_ids(&wb.pkg)?);
        wb.comment_guids_scanned = true;
    }
    for _ in 0..32 {
        let mut bytes = random_bytes(wb)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p] = bytes;
        let id = format!(
            "{{{a:02X}{b:02X}{c:02X}{d:02X}-{e:02X}{f:02X}-{g:02X}{h:02X}-{i:02X}{j:02X}-{k:02X}{l:02X}{m:02X}{n:02X}{o:02X}{p:02X}}}"
        );
        if !wb.comment_guids.contains(&id) {
            return Ok(id);
        }
    }
    Err(Error::Random(
        "could not allocate a unique comment id".into(),
    ))
}

pub(super) fn read(pkg: &Package, sheet: &str) -> Result<Option<(String, XmlDocument)>> {
    xml::target(pkg, sheet, REL)?
        .map(|part| Ok((part.clone(), xml::read(pkg, &part, ROOT)?)))
        .transpose()
}

pub(super) fn at<'doc>(
    part: &str,
    doc: &'doc XmlDocument,
    at: CellIndex,
) -> Result<Vec<&'doc RawNode>> {
    xml::ordered(doc.root(), ROOT, &[ENTRY, EXT], part)?;
    let mut entries = Vec::new();
    for node in doc.root().elements().filter(|node| node.is(ENTRY)) {
        let reference = node
            .attr(attr("ref"))
            .and_then(|at| CellIndex::parse(at).ok())
            .ok_or_else(|| part_error(part, "invalid threaded comment reference"))?;
        if reference == at {
            entries.push(node);
        }
    }
    Ok(entries)
}

pub(super) fn root_id<'doc>(part: &str, entries: &[&'doc RawNode]) -> Result<&'doc str> {
    let mut roots = entries
        .iter()
        .filter(|node| node.attr(attr("parentId")).is_none());
    let Some(id) = roots
        .next()
        .and_then(|node| node.attr(attr("id")))
        .filter(|id| !id.is_empty())
    else {
        return Err(part_error(part, "thread has no root id"));
    };
    if roots.next().is_some() {
        return Err(part_error(part, "multiple threads match the cell"));
    }
    Ok(id)
}

pub(super) fn fallback_text(part: &str, entries: &[&RawNode], reply: &str) -> Result<String> {
    let mut text = String::from(FALLBACK);
    for node in entries {
        let value =
            child(node, TEXT).ok_or_else(|| part_error(part, "thread entry has no text"))?;
        text.push_str(&escape::decode(&value.text()));
        text.push_str("\n\n");
    }
    text.push_str(reply);
    Ok(text)
}

pub(super) fn append(
    pkg: &Package,
    plan: &mut RegistryPlan,
    sheet: &str,
    existing: Option<&(String, XmlDocument)>,
    last: Option<&RawNode>,
    markup: String,
) -> Result<()> {
    if let Some((part, doc)) = existing {
        xml::require_owner(pkg, part, sheet)?;
        let root = doc.root();
        let patch = if let Some(last) = last {
            let end = last.span().expect("parsed entry").end();
            (end..end, markup)
        } else {
            xml::insert(root, child(root, EXT), markup)
        };
        xml::stage(pkg, plan, part, doc, vec![patch])
    } else {
        xml::add(
            pkg,
            plan,
            sheet,
            REL,
            PartNameRequest::Numbered("xl/threadedComments/threadedComment{}.xml".into()),
            TYPE,
            format!("<ThreadedComments xmlns=\"{NS}\">{markup}</ThreadedComments>"),
        )?;
        Ok(())
    }
}
