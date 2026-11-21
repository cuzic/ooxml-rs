//! Span edits of small parts, retaining every byte outside the edited spans.

use std::ops::Range;

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{Package, PartNameRequest, QName, RawNode, RegistryPlan, Target, XmlDocument};

use crate::Result;
use crate::images::part_error;

pub(super) const RID: QName = QName::new(
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "id",
);

pub(super) const fn attr(name: &'static str) -> QName {
    QName::new("", name)
}

pub(super) fn default_namespace(node: &RawNode) -> Option<&str> {
    node.attr(QName::new("http://www.w3.org/2000/xmlns/", "xmlns"))
}

pub(super) fn namespace_declaration(bound: Option<&str>, expected: &str) -> String {
    if bound == Some(expected) {
        String::new()
    } else {
        format!(" xmlns=\"{expected}\"")
    }
}

pub(super) fn child(node: &RawNode, name: QName) -> Option<&RawNode> {
    node.elements().find(|node| node.is(name))
}

pub(super) fn target(pkg: &Package, source: &str, kind: &str) -> Result<Option<String>> {
    let rels = pkg.relationships(source)?;
    let mut matches = rels.by_type(kind);
    let Some(rel) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(part_error(source, format!("multiple {kind} relationships")));
    }
    rels.resolve(rel)
        .and_then(|part| pkg.part_name(&part).map(str::to_owned))
        .map(Some)
        .ok_or_else(|| part_error(source, format!("unresolved {kind} relationship")))
}

pub(super) fn require_owner(pkg: &Package, part: &str, owner: &str) -> Result<()> {
    let links = pkg.incoming_relationships(part)?;
    if links.len() != 1 || links[0].source.as_deref() != Some(owner) {
        return Err(part_error(part, "part ownership is ambiguous"));
    }
    Ok(())
}

pub(super) fn read(pkg: &Package, part: &str, root: QName) -> Result<XmlDocument> {
    let bytes = pkg
        .read_part(part)
        .map_err(|error| part_error(part, error))?;
    let doc = XmlDocument::parse_with(&bytes, &pkg.parse_options())
        .map_err(|error| part_error(part, error))?;
    if !doc.root().is(root) {
        return Err(part_error(part, format!("expected {}", root.local)));
    }
    Ok(doc)
}

pub(super) type Patch = (Range<usize>, String);

pub(super) fn replace(node: &RawNode, replacement: String) -> Patch {
    let span = node.span().expect("parsed node");
    (span.start..span.end(), replacement)
}

pub(super) fn insert(parent: &RawNode, before: Option<&RawNode>, markup: String) -> Patch {
    if let Some(before) = before {
        let start = before.span().expect("parsed sibling").start;
        return (start..start, markup);
    }
    let span = parent.span().expect("parsed parent");
    if let Some((close, _)) = span.close {
        return (close..close, markup);
    }
    let name = parent.name();
    let local = name.local();
    let qualified = name
        .prefix()
        .map_or_else(|| local.to_owned(), |p| format!("{p}:{local}"));
    (
        span.open_end - 2..span.open_end,
        format!(">{markup}</{qualified}>"),
    )
}

pub(super) fn ordered(parent: &RawNode, name: QName, allowed: &[QName], part: &str) -> Result<()> {
    for node in parent.elements() {
        if node.is(MC_ALTERNATE_CONTENT) {
            return Err(ooxml_core::Error::UnresolvedSibling(name).into());
        }
        if !allowed.iter().any(|name| node.is(*name)) {
            return Err(part_error(
                part,
                format!("unexpected {} child", node.name().local()),
            ));
        }
    }
    Ok(())
}

pub(super) fn stage(
    pkg: &Package,
    plan: &mut RegistryPlan,
    part: &str,
    doc: &XmlDocument,
    edits: Vec<Patch>,
) -> Result<()> {
    if edits.is_empty() {
        return Ok(());
    }
    let result = render(doc, edits);
    let tree = XmlDocument::parse_with(result.as_bytes(), &pkg.parse_options())
        .map_err(|error| part_error(part, error))?;
    plan.stage_part(pkg, part, tree)?;
    Ok(())
}

pub(super) fn render(doc: &XmlDocument, mut edits: Vec<Patch>) -> String {
    edits.sort_by_key(|(range, _)| range.start);
    let mut result = doc.source().to_owned();
    for (range, markup) in edits.into_iter().rev() {
        result.replace_range(range, &markup);
    }
    result
}

pub(super) fn add(
    pkg: &Package,
    plan: &mut RegistryPlan,
    source: &str,
    kind: &str,
    name: PartNameRequest,
    content_type: &str,
    markup: String,
) -> Result<String> {
    XmlDocument::parse_with(markup.as_bytes(), &pkg.parse_options())
        .map_err(|error| part_error(source, error))?;
    let part = plan.add_part(pkg, name, content_type, markup.into_bytes())?;
    Ok(plan.add_relationship(pkg, Some(source), kind, Target::Part(part))?)
}

pub(super) fn text(value: &str) -> String {
    // XML parsers normalize raw CR. Use a reference for an authored CR.
    quick_xml::escape::escape(value).replace('\r', "&#13;")
}

pub(super) fn attribute(value: &str) -> String {
    text(value).replace('\n', "&#10;").replace('\t', "&#9;")
}
