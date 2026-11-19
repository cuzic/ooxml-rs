//! The text model shared by the oracles (ADR-0005 §7 T3): every slide in
//! `p:sldIdLst` order, every `p:sp` with a `p:txBody` and every table cell,
//! depth-first, outside `mc:AlternateContent`; per paragraph the **format
//! form** of `a:pPr` / `a:endParaRPr` and the items (`["r", text, rPr
//! form]`, `["br"]`, `["fld", text]`); per slide also the paragraphs of
//! its notes body (`notes`, `null` without one; ADR-0008 §2). `tests/oracle/readback.py` produces
//! the same JSON with python-pptx; `apply` in `tests/oracle.rs` computes the
//! expected model after a list of edits (`tests/oracle/check.py` computes it
//! independently, by editing python-pptx's own tree).
//!
//! The format form (review R2-2) is the whole subtree, not only the
//! attributes: `{"attrs": {...}, "children": [{"name", "attrs",
//! "children"[, "text"]}...]}` with Clark names, attributes sorted (a JSON
//! object), namespace declarations left out, child elements in document
//! order, and `text` only on elements without element children. `null`
//! when the element is absent or has neither attributes nor children.

#![allow(dead_code)]

use ooxml_core::RawNode;
use ooxml_pptx::{Presentation, ShapeKind, TextBodyRef, TextItem};
use serde_json::{Map, Value, json};

fn clark(ns: &str, local: &str) -> String {
    if ns.is_empty() { local.to_owned() } else { format!("{{{ns}}}{local}") }
}

/// Attributes as a JSON object (Clark keys), namespace declarations left out.
pub fn attr_map(n: &RawNode) -> Map<String, Value> {
    n.attrs()
        .iter()
        .filter(|a| !a.name().is_namespace_declaration())
        .map(|a| (clark(a.name().ns(), a.name().local()), Value::String(a.value().to_owned())))
        .collect()
}

/// Canonical form of an element subtree.
pub fn canon(n: &RawNode) -> Value {
    let children: Vec<Value> = n.elements().map(canon).collect();
    let mut v = json!({"name": clark(n.name().ns(), n.name().local()), "attrs": attr_map(n), "children": children});
    if n.elements().next().is_none() && !n.text().is_empty() {
        v["text"] = json!(n.text());
    }
    v
}

/// Format form of a formatting element (`a:rPr`, `a:pPr`, `a:endParaRPr`).
pub fn fmt(n: Option<&RawNode>) -> Value {
    let Some(n) = n else { return Value::Null };
    let attrs = attr_map(n);
    let children: Vec<Value> = n.elements().map(canon).collect();
    if attrs.is_empty() && children.is_empty() {
        return Value::Null;
    }
    json!({"attrs": attrs, "children": children})
}

/// The attribute-only projection of a format form (the round-2 model):
/// used to show what the richer form catches.
pub fn attrs_only(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.contains_key("children") && m.contains_key("attrs") && !m.contains_key("name") => {
            if m["attrs"].as_object().is_some_and(Map::is_empty) { Value::Null } else { m["attrs"].clone() }
        }
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), attrs_only(x))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(attrs_only).collect()),
        x => x.clone(),
    }
}

fn child<'a>(n: &'a RawNode, local: &str) -> Option<&'a RawNode> {
    n.elements().find(|c| c.name().local() == local)
}

pub fn body(b: TextBodyRef<'_>) -> Value {
    let paras: Vec<Value> = b
        .paragraphs()
        .iter()
        .map(|p| {
            let items: Vec<Value> = p
                .items()
                .iter()
                .filter_map(|it| match it {
                    TextItem::Run(r) => Some(json!(["r", r.text(), fmt(child(r.raw(), "rPr"))])),
                    TextItem::Break => Some(json!(["br"])),
                    TextItem::Field(f) => Some(json!(["fld", f.text()])),
                    _ => None,
                })
                .collect();
            json!({
                "ppr": fmt(child(p.raw(), "pPr")),
                "end": fmt(child(p.raw(), "endParaRPr")),
                "items": items,
            })
        })
        .collect();
    Value::Array(paras)
}

pub fn dump(pres: &mut Presentation) -> Value {
    let mut slides = Vec::new();
    for i in 0..pres.slide_count() {
        let slide = pres.slide(i).expect("slide");
        let mut shapes = Vec::new();
        for s in slide.shapes() {
            let Ok(id) = s.id() else { continue };
            match s.kind() {
                ShapeKind::Shape => {
                    if let Some(b) = s.text_body() {
                        let ph = s.placeholder().map(|p| {
                            let r = p.view().raw();
                            json!([r.attr(ooxml_core::QName::new("", "type")), r.attr(ooxml_core::QName::new("", "idx"))])
                        });
                        shapes.push(json!({"shape_id": id, "name": s.name(), "cell": null, "ph": ph, "paragraphs": body(b)}));
                    }
                }
                ShapeKind::Table => {
                    let t = s.table().expect("table");
                    for (r, row) in t.rows().iter().enumerate() {
                        for (c, cell) in row.iter().enumerate() {
                            let paragraphs = cell.text_body().map_or(Value::Array(vec![]), body);
                            shapes.push(json!({"shape_id": id, "name": s.name(), "cell": [r, c], "ph": null, "paragraphs": paragraphs}));
                        }
                    }
                }
                _ => {}
            }
        }
        let info = slide.info().clone();
        // the notes body (ADR-0008 §2): python-pptx `notes_placeholder`
        let notes = pres.notes(i).expect("notes").and_then(|n| n.body().map(body)).unwrap_or(Value::Null);
        slides.push(json!({"slide_id": info.slide_id, "partname": format!("/{}", info.part_name), "shapes": shapes, "notes": notes}));
    }
    json!({ "slides": slides })
}
