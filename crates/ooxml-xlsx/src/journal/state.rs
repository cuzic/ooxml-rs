//! Cell state equality (ADR-0024 §4.4, raw): what "back in its snapshot
//! state" compares. A stand-in for C8's typed cell-state reads.

use std::collections::BTreeMap;

use ooxml_core::{QName, RawItem, RawNode};

use crate::error::Result;
use crate::workbook::{REL_SHARED_STRINGS, Workbook};

/// Order-insensitive attributes, element children in order, text;
/// whitespace-only text between elements dropped; spans and dirty flags ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tree {
    El {
        name: String,
        attrs: BTreeMap<String, String>,
        children: Vec<Tree>,
    },
    Text(String),
}

pub(crate) fn tree(node: &RawNode) -> Tree {
    let attrs = node
        .attrs()
        .iter()
        .map(|a| {
            let n = a.name();
            let key = if n.ns().is_empty() {
                n.local().to_owned()
            } else {
                format!("{{{}}}{}", n.ns(), n.local())
            };
            (key, a.value().to_owned())
        })
        .collect();
    let has_elements = node.elements().next().is_some();
    let children = node
        .children()
        .iter()
        .filter_map(|c| match c {
            RawItem::Element(e) => Some(tree(e)),
            RawItem::Text(t) if has_elements && t.is_whitespace() => None,
            RawItem::Text(t) => Some(Tree::Text(t.value().to_owned())),
            RawItem::Markup(_) => None,
        })
        .collect();
    let n = node.name();
    let name = if n.ns().is_empty() {
        n.local().to_owned()
    } else {
        format!("{{{}}}{}", n.ns(), n.local())
    };
    Tree::El {
        name,
        attrs,
        children,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellState {
    pub(crate) present: bool,
    /// Missing `s` = 0.
    pub(crate) s: u32,
    pub(crate) content: Content,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Content {
    /// Also every missing cell (§4.4).
    Empty,
    /// `t` (or `"n"`) and the literal `v` text.
    Value { t: String, v: String },
    /// `t="s"`: the `si` item it points at, runs included.
    Shared(Tree),
    /// `t="inlineStr"`: the `is` element.
    Inline(Tree),
    /// Every `f` attribute, the text, and `cm`. The cache is not compared.
    Formula {
        attrs: BTreeMap<String, String>,
        text: String,
        cm: Option<String>,
    },
}

fn attr<'a>(n: &'a RawNode, name: &'static str) -> Option<&'a str> {
    n.attr(QName::new("", name))
}

fn child<'a>(n: &'a RawNode, local: &str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.name().local() == local)
}

/// The cell state of `node` (`None` = the cell does not exist).
pub(crate) fn state_of(
    wb: &mut Workbook,
    _sheet: usize,
    node: Option<&RawNode>,
) -> Result<CellState> {
    let Some(c) = node else {
        return Ok(CellState {
            present: false,
            s: 0,
            content: Content::Empty,
        });
    };
    let s = attr(c, "s")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let t = attr(c, "t");
    let content = if let Some(f) = child(c, "f") {
        let attrs = f
            .attrs()
            .iter()
            .map(|a| (a.name().local().to_owned(), a.value().to_owned()))
            .collect();
        Content::Formula {
            attrs,
            text: f.text().into_owned(),
            cm: attr(c, "cm").map(str::to_owned),
        }
    } else if t == Some("inlineStr")
        && let Some(is) = child(c, "is")
    {
        Content::Inline(tree(is))
    } else if let Some(v) = child(c, "v") {
        let text = v.text().into_owned();
        match (t, text.trim().parse::<usize>()) {
            (Some("s"), Ok(i)) => match shared_item(wb, i)? {
                Some(t) => Content::Shared(t),
                None => Content::Value {
                    t: "s".into(),
                    v: text,
                },
            },
            _ => Content::Value {
                t: t.unwrap_or("n").to_owned(),
                v: text,
            },
        }
    } else {
        Content::Empty
    };
    Ok(CellState {
        present: true,
        s,
        content,
    })
}

/// The `i`th `si` of the shared-string table, as it is now.
fn shared_item(wb: &mut Workbook, i: usize) -> Result<Option<Tree>> {
    let rels = wb.pkg.relationships(&wb.workbook_part)?;
    let Some(part) = rels
        .by_type(REL_SHARED_STRINGS)
        .next()
        .and_then(|r| rels.resolve(r))
        .filter(|p| wb.pkg.contains(p))
    else {
        return Ok(None);
    };
    let doc = wb.pkg.xml_part(&part)?;
    Ok(doc
        .root()
        .elements()
        .filter(|e| e.name().local() == "si")
        .nth(i)
        .map(tree))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(xml: &str) -> RawNode {
        ooxml_core::XmlDocument::parse(xml.as_bytes())
            .unwrap()
            .root()
            .clone()
    }

    #[test]
    fn tree_ignores_attribute_order_and_inter_element_whitespace() {
        let a = tree(&node(r#"<si a="1" b="2"> <r><t>x</t></r> </si>"#));
        let b = tree(&node(r#"<si b="2" a="1"><r><t>x</t></r></si>"#));
        assert_eq!(a, b);
        assert_ne!(a, tree(&node(r#"<si b="2" a="1"><r><t>y</t></r></si>"#)));
        // whitespace-only text is data when it is the content
        assert_ne!(tree(&node("<t> </t>")), tree(&node("<t></t>")));
    }
}
