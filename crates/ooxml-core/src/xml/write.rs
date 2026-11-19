//! Layer 2 serializer (ADR-0001 "Serialization rule").

use std::fmt::Write as _;

use super::{RawItem, RawNode, XmlDocument};

pub(super) fn serialize(doc: &XmlDocument) -> String {
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
    // An originally self-closing element with children must be re-rendered
    // even if a caller bypassed the duty that marks it `own_dirty`.
    let rerender = node.own_dirty()
        || match span {
            None => true,
            Some(s) => s.close.is_none() && !node.children().is_empty(),
        };
    if rerender {
        render_open_tag(out, node, node.children().is_empty());
    } else {
        let s = span.expect("checked above");
        out.push_str(&src[s.start..s.open_end]);
    }
    for child in node.children() {
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
    if rerender {
        if !node.children().is_empty() {
            let _ = write!(out, "</{}>", node.name.display());
        }
    } else if let Some((cs, ce)) = span.and_then(|s| s.close) {
        out.push_str(&src[cs..ce]);
    }
}

fn render_open_tag(out: &mut String, node: &RawNode, self_closing: bool) {
    let _ = write!(out, "<{}", node.name.display());
    for a in &node.attrs {
        let _ = write!(out, " {}=\"", a.name.display());
        escape_attr(out, &a.value);
        out.push('"');
    }
    out.push_str(if self_closing { "/>" } else { ">" });
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
