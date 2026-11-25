//! Layer 2 serializer (ADR-0001 "Serialization rule").

use std::fmt::Write as _;

use quick_xml::{XmlVersion, events::BytesStart};

use super::{RawAttr, RawItem, RawNode, XmlDocument};
use crate::name::XName;

pub(super) fn serialize(doc: &XmlDocument) -> String {
    debug_assert!(doc.root.dirty_counts_consistent(), "a dirty-child count is out of step with the tree");
    let src = doc.source();
    let mut out = String::with_capacity(src.len() + 64);
    let root = &doc.root;
    let span = root.span().expect("the root element is always parsed");
    out.push_str(&src[..span.start]);
    write_node(&mut out, src, root);
    out.push_str(&src[span.end()..]);
    out
}

fn write_node(out: &mut String, src: &str, node: &RawNode) {
    let span = match node.span() {
        Some(span) if !node.subtree_dirty() => {
            out.push_str(&src[span.start..span.end()]);
            return;
        }
        other => other,
    };
    let deferred = node.deferred_content(src);
    let empty = || deferred.map_or_else(|| node.children().is_empty(), str::is_empty);
    // An originally self-closing element with children must be re-rendered
    // even if a caller bypassed the duty that marks it `own_dirty`.
    let rerender = node.own_dirty()
        || match span {
            None => true,
            Some(s) => s.close.is_none() && !empty(),
        };
    let preserved = rerender
        && node.preserves_attribute_spelling()
        && span
            .is_some_and(|s| render_preserved_tag(out, &src[s.start..s.open_end], node, empty).is_some());
    if rerender {
        if !preserved {
            render_open_tag(out, node, empty());
        }
    } else {
        let s = span.expect("checked above");
        out.push_str(&src[s.start..s.open_end]);
    }
    if let Some(content) = deferred {
        out.push_str(content);
    } else {
        for child in node.children() {
            write_child(out, src, child);
        }
    }
    match span.and_then(|s| s.close) {
        Some((start, end)) if preserved || !rerender => out.push_str(&src[start..end]),
        _ if rerender && !empty() => {
            let _ = write!(out, "</{}>", node.name.display());
        }
        _ => {}
    }
}

fn write_child(out: &mut String, src: &str, child: &RawItem) {
    match child {
        RawItem::Element(n) => write_node(out, src, n),
        RawItem::Text(t) => match t.span() {
            Some(r) => out.push_str(&src[r]),
            None if t.cdata => {
                // `]]>` cannot occur inside a CDATA section: split it.
                out.push_str("<![CDATA[");
                out.push_str(&t.value.replace("]]>", "]]]]><![CDATA[>"));
                out.push_str("]]>");
            }
            None => escape_text(out, &t.value),
        },
        RawItem::Markup(m) => out.push_str(&m.raw),
    }
}

fn render_open_tag(out: &mut String, node: &RawNode, self_closing: bool) {
    let _ = write!(out, "<{}", node.name.display());
    for a in &node.attrs {
        render_attr(out, a);
    }
    out.push_str(if self_closing { "/>" } else { ">" });
}

fn render_attr(out: &mut String, attr: &RawAttr) {
    let _ = write!(out, " {}=\"", attr.name.display());
    escape_attr(out, &attr.value);
    out.push('"');
}

fn render_preserved_tag(out: &mut String, original: &str, node: &RawNode, empty: impl FnOnce() -> bool) -> Option<()> {
    let tag = original.strip_prefix('<')?.strip_suffix('>')?;
    let self_closing = tag.ends_with('/');
    let body = tag.strip_suffix('/').unwrap_or(tag);
    let name_end = body.find([' ', '\t', '\r', '\n']).unwrap_or(body.len());
    if !spells_name(&body[..name_end], &node.name) {
        return None;
    }
    let tag = BytesStart::from_content(body, name_end);
    let mut previous_end = name_end;
    let mut spellings = Vec::new();
    for attr in tag.attributes() {
        let attr = attr.ok()?;
        let key = attr.key.into_inner();
        let value_end = body.substr_range(attr.value.as_ref())?.end + 1;
        let value = attr.normalized_value(XmlVersion::Implicit1_0).ok()?;
        spellings.push((key, value, &body[previous_end..value_end]));
        previous_end = value_end;
    }

    out.push_str(&original[..name_end + 1]);
    for attr in node.attrs() {
        if let Some((_, _, spelling)) = spellings
            .iter()
            .find(|(key, value, _)| spells_name(key, &attr.name) && value.as_ref() == attr.value())
        {
            out.push_str(spelling);
        } else {
            render_attr(out, attr);
        }
    }
    out.push_str(&body[previous_end..]);
    out.push_str(if self_closing && empty() {
        "/>"
    } else {
        ">"
    });
    Some(())
}

fn spells_name(raw: &str, name: &XName) -> bool {
    match raw.split_once(':') {
        Some((prefix, local)) => name.prefix() == Some(prefix) && name.local() == local,
        None => name.prefix().is_none() && name.local() == raw,
    }
}

fn escape_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}
