use std::collections::{BTreeMap, BTreeSet};

use ooxml_core::{LeafStep, QName, RawNode, XmlDocument};

use crate::fields::FieldScan;
use crate::names::w;
use crate::{Document, Error, Result};

const CORE_ID: &str = "{6C3C8BC8-F283-45AE-878A-BAB7291924A1}";
const APP_ID: &str = "{6668398D-A668-4E3E-A5EB-62B293D839F1}";
const CORE_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const CORE_NS: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const CUSTOM_XML_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";
const ITEM_PROPS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
const DS_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/customXml";

/// Word's XML data binding attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Custom XML store item identifier.
    pub store_item_id: String,
    /// Absolute element XPath, optionally with positional predicates.
    pub xpath: String,
    /// Namespace declarations for XPath prefixes.
    pub prefix_mappings: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Target {
    id: String,
    path: Vec<(String, String, usize)>,
}

pub(super) fn read(pr: &RawNode) -> Result<Option<Binding>> {
    let mut binding = None;
    for node in pr.elements().filter(|n| n.name().local() == "dataBinding") {
        if !node.is(w("dataBinding")) || binding.is_some() {
            return Err(Error::UnsupportedBinding);
        }
        binding = Some(binding_attributes(node).ok_or(Error::UnsupportedBinding)?);
    }
    Ok(binding)
}

fn binding_attributes(node: &RawNode) -> Option<Binding> {
    let attr = |local| {
        node.attr(w(local)).or_else(|| {
            node.attrs()
                .iter()
                .find(|attr| attr.name().ns() == node.name().ns() && attr.name().local() == local)
                .map(ooxml_core::RawAttr::value)
        })
    };
    Some(Binding {
        store_item_id: attr("storeItemID")?.into(),
        xpath: attr("xpath")?.into(),
        prefix_mappings: attr("prefixMappings").unwrap_or_default().into(),
    })
}

pub(super) fn matches_target(pr: &RawNode, target: &Target) -> Result<bool> {
    let matching = pr
        .elements()
        .filter(|node| node.name().local() == "dataBinding")
        .filter_map(binding_attributes)
        .any(|binding| binding.target().is_ok_and(|candidate| candidate == *target));
    if matching {
        read(pr)?;
    }
    Ok(matching)
}

impl Binding {
    pub(super) fn target(&self) -> Result<Target> {
        let prefixes = parse_prefixes(&self.prefix_mappings).ok_or(Error::UnsupportedBinding)?;
        let path = self
            .xpath
            .strip_prefix('/')
            .ok_or(Error::UnsupportedBinding)?
            .split('/')
            .map(|step| parse_step(step, &prefixes).ok_or(Error::UnsupportedBinding))
            .collect::<Result<Vec<_>>>()?;
        Ok(Target {
            id: self.store_item_id.to_ascii_uppercase(),
            path,
        })
    }
}

fn parse_prefixes(mut text: &str) -> Option<BTreeMap<&str, &str>> {
    let mut prefixes = BTreeMap::new();
    while !text.trim_start().is_empty() {
        let (declaration, value) = text.trim_start().strip_prefix("xmlns:")?.split_once('=')?;
        let declaration = declaration.trim_end();
        let value = value.trim_start();
        let quote = value.chars().next()?;
        if !matches!(quote, '\'' | '"') || !ncname(declaration) {
            return None;
        }
        let (uri, rest) = value[1..].split_once(quote)?;
        if uri.is_empty() || prefixes.insert(declaration, uri).is_some() {
            return None;
        }
        text = rest;
    }
    Some(prefixes)
}

fn ncname(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| matches!(c, '_' | '-' | '.') || c.is_alphanumeric())
}

fn parse_step(step: &str, prefixes: &BTreeMap<&str, &str>) -> Option<(String, String, usize)> {
    let (name, index) = match step.split_once('[') {
        Some((name, position)) => {
            let digits = position.strip_suffix(']')?;
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (name, digits.parse::<usize>().ok()?.checked_sub(1)?)
        }
        None => (step, 0),
    };
    let (uri, name) = if let Some((prefix, local)) = name.split_once(':') {
        (*prefixes.get(prefix)?, local)
    } else {
        ("", name)
    };
    ncname(name).then(|| (uri.into(), name.into(), index))
}

impl Target {
    fn steps(&self) -> Vec<LeafStep<'_>> {
        self.path
            .iter()
            .map(|(ns, local, index)| LeafStep {
                ns,
                local,
                index: *index,
            })
            .collect()
    }

    fn core_property(&self) -> Result<Option<(&str, &str)>> {
        if self.id != CORE_ID {
            return Ok(None);
        }
        let [(root_ns, root, 0), (ns, name, 0)] = self.path.as_slice() else {
            return Err(Error::UnsupportedBinding);
        };
        if root_ns != CORE_NS || root != "coreProperties" {
            return Err(Error::UnsupportedBinding);
        }
        let property = match (ns.as_str(), name.as_str()) {
            (DC_NS, "title") => ("TITLE", "TITLE"),
            (DC_NS, "subject") => ("SUBJECT", "SUBJECT"),
            (DC_NS, "creator") => ("AUTHOR", "AUTHOR"),
            (CORE_NS, "keywords") => ("KEYWORDS", "KEYWORDS"),
            (DC_NS, "description") => ("COMMENTS", "COMMENTS"),
            (CORE_NS, "category") => ("CATEGORY", ""),
            (CORE_NS, "contentStatus") => ("CONTENTSTATUS", ""),
            _ => return Err(Error::UnsupportedBinding),
        };
        Ok(Some(property))
    }

    pub(super) fn store(&self, doc: &Document) -> Result<(String, XmlDocument)> {
        let part = match self.id.as_str() {
            CORE_ID => {
                self.core_property()?;
                let rels = doc.pkg.package_relationships();
                let mut targets = rels.by_type(CORE_REL);
                let part = targets
                    .next()
                    .and_then(|rel| rels.resolve(rel))
                    .ok_or(Error::UnsupportedBinding)?;
                if targets.next().is_some() {
                    return Err(Error::UnsupportedBinding);
                }
                part
            }
            APP_ID => return Err(Error::UnsupportedBinding),
            _ => self.custom_part(doc)?,
        };
        let xml = load(doc, &part)?;
        Ok((part, xml))
    }

    fn custom_part(&self, doc: &Document) -> Result<String> {
        let main = &doc.stories[0].id.part;
        let rels = doc
            .pkg
            .relationships(main)
            .map_err(|_| Error::UnsupportedBinding)?;
        let mut matches = BTreeSet::new();
        for part in rels
            .by_type(CUSTOM_XML_REL)
            .filter_map(|rel| rels.resolve(rel))
        {
            let Ok(props) = doc.pkg.relationships(&part) else {
                continue;
            };
            for target in props
                .by_type(ITEM_PROPS_REL)
                .filter_map(|rel| props.resolve(rel))
            {
                let Ok(xml) = load(doc, &target) else {
                    continue;
                };
                if xml.root().is(QName::new(DS_NS, "datastoreItem"))
                    && xml
                        .root()
                        .attr(QName::new(DS_NS, "itemID"))
                        .is_some_and(|id| id.eq_ignore_ascii_case(&self.id))
                {
                    matches.insert(part.clone());
                }
            }
        }
        if matches.len() != 1 {
            return Err(Error::UnsupportedBinding);
        }
        matches.pop_first().ok_or(Error::UnsupportedBinding)
    }

    pub(super) fn value(&self, xml: &XmlDocument) -> Result<String> {
        let steps = self.steps();
        xml.check_set_leaf_text(&steps, "")
            .map_err(|_| Error::UnsupportedBinding)?;
        let mut node = xml.root();
        for step in &steps[1..] {
            node = node
                .elements()
                .filter(|n| n.name().ns() == step.ns && n.name().local() == step.local)
                .nth(step.index)
                .ok_or(Error::UnsupportedBinding)?;
        }
        Ok(node.text().into_owned())
    }

    pub(super) fn write(&self, doc: &Document, xml: &mut XmlDocument, value: &str) -> Result<()> {
        if let Some((property, shorthand)) = self.core_property()?
            && !doc.fields_opt_in
        {
            let key = format!("DOCPROPERTY {property}");
            let codes: Vec<_> = doc
                .stories()
                .flat_map(|story| FieldScan::scan(story.root))
                .filter(|code| {
                    code == &key
                        || code == shorthand
                        || (!shorthand.is_empty() && code.strip_prefix("INFO ") == Some(shorthand))
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            if !codes.is_empty() {
                return Err(Error::StaleFieldRisk { codes });
            }
        }
        let steps = self.steps();
        xml.check_set_leaf_text(&steps, value)
            .map_err(|_| Error::UnsupportedBinding)?;
        xml.set_leaf_text(&steps, value)
            .map_err(|_| Error::UnsupportedBinding)
    }
}

fn load(doc: &Document, name: &str) -> Result<XmlDocument> {
    if let Some(xml) = doc.pkg.parsed_part(name) {
        return Ok(xml.clone());
    }
    let bytes = doc
        .pkg
        .read_part(name)
        .map_err(|_| Error::UnsupportedBinding)?;
    XmlDocument::parse(&bytes).map_err(|_| Error::UnsupportedBinding)
}
