//! Cell state equality (ADR-0024 §4.4): what "back in its snapshot state"
//! compares, on C8's typed reads (`cell_state.rs`) where they are
//! equivalent. Three raw reads remain (C8 follow-ups): the plain text of a
//! string item, `cm`/`vm`, and a value's literal.

use std::collections::BTreeMap;

use ooxml_core::{QName, RawItem, RawNode};

use crate::cell_state::{FormulaState, StringState};
use crate::error::Result;
use crate::reference::CellIndex;
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
    /// `t` (or `"n"`) and the literal `v` text. Also a shared string whose
    /// item C8 cannot read (same index, same item: `sst` is append-only).
    Value { t: String, v: String },
    /// A string cell (`t="s"` or `t="inlineStr"`): the text of its item as
    /// written, plus C8's runs and phonetics. Shared and inline strings of
    /// equal text are different states.
    Str {
        inline: bool,
        text: String,
        string: StringState,
    },
    /// C8's typed `f` (every attribute, the text) and `cm`. The cache is not
    /// compared.
    Formula {
        f: FormulaState,
        cm: Option<String>,
    },
}

fn attr<'a>(n: &'a RawNode, name: &'static str) -> Option<&'a str> {
    n.attr(QName::new("", name))
}

fn child<'a>(n: &'a RawNode, local: &str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.name().local() == local)
}

/// The text of a string item (`si` or `is`) as written: its own `t` and
/// those of its runs, phonetic text left out (C8 compares that). C8 gap 1:
/// it has no plain text.
fn item_text(item: &RawNode) -> String {
    let mut out = String::new();
    for e in item.elements() {
        match e.name().local() {
            "t" => out.push_str(&e.text()),
            "r" => {
                for t in e.elements().filter(|t| t.name().local() == "t") {
                    out.push_str(&t.text());
                }
            }
            _ => {}
        }
    }
    out
}

/// The cell state of `node` (`None` = the cell does not exist); `at` names
/// the cell in C8's errors.
pub(crate) fn state_of(
    wb: &mut Workbook,
    _sheet: usize,
    at: CellIndex,
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
    let content = if let Some(f) = FormulaState::of(c) {
        // C8 gap 2: no `cm`/`vm` read
        Content::Formula {
            f,
            cm: attr(c, "cm").map(str::to_owned),
        }
    } else if t == Some("inlineStr")
        && let Some(is) = child(c, "is")
        && let Ok(Some(string)) = wb.string_state_of(c, at)
    {
        Content::Str {
            inline: true,
            text: item_text(is),
            string,
        }
    } else if let Some(v) = child(c, "v") {
        let text = v.text().into_owned();
        // C8 gap 3: no value read, `v` stays raw
        let raw = || Content::Value {
            t: t.unwrap_or("n").to_owned(),
            v: text.clone(),
        };
        match (t, text.trim().parse::<usize>()) {
            (Some("s"), Ok(i)) => match (wb.string_state_of(c, at), shared_text(wb, i)?) {
                (Ok(Some(string)), Some(text)) => Content::Str {
                    inline: false,
                    text,
                    string,
                },
                // a dangling index, or `sst` behind `mc:AlternateContent`
                _ => raw(),
            },
            _ => raw(),
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

/// The text of the `i`th `si` of the shared-string table, as it is now.
fn shared_text(wb: &mut Workbook, i: usize) -> Result<Option<String>> {
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
        .map(item_text))
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

    fn state(c: &str) -> CellState {
        let mut wb = crate::workbook::Workbook::new().unwrap();
        let node = node(c);
        state_of(&mut wb, 0, CellIndex { row: 0, col: 0 }, Some(&node)).unwrap()
    }

    #[test]
    fn cm_is_part_of_a_formula_state() {
        let a = state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1" cm="1"><f t="array" ref="A1">1+1</f><v>2</v></c>"#);
        let b = state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1"><f t="array" ref="A1">1+1</f><v>2</v></c>"#);
        assert_ne!(a, b);
        // value equality of booleans, the cache is not compared
        let c = state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1"><f ca="true">1+1</f><v>2</v></c>"#);
        let d = state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1"><f ca="1">1+1</f><v>3</v></c>"#);
        assert_eq!(c, d);
        assert_ne!(c, state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1"><f>1+1</f></c>"#));
    }

    #[test]
    fn a_string_state_is_its_text_runs_and_kind() {
        let plain = state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1" t="inlineStr"><is><t>ab</t></is></c>"#);
        let rich = state(r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="A1" t="inlineStr"><is><r><t>a</t></r><r><t>b</t></r></is></c>"#);
        assert_ne!(plain, rich, "runs");
        let Content::Str { text, .. } = &rich.content else {
            panic!("{rich:?}")
        };
        assert_eq!(text, "ab");
    }
}
