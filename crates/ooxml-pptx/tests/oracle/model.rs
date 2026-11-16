//! The text model shared by the oracles (ADR-0005 §7 T3): every slide in
//! `p:sldIdLst` order, every `p:sp` with a `p:txBody` and every table cell,
//! depth-first, outside `mc:AlternateContent`; per paragraph the `a:pPr` /
//! `a:endParaRPr` attributes and the items (`["r", text, rPr attrs]`,
//! `["br"]`, `["fld", text]`). `tests/oracle/readback.py` produces the same
//! JSON with python-pptx; `apply` (Rust: `tests/oracle.rs`, Python:
//! `check.py`) computes the expected model after a list of edits.

#![allow(dead_code)]

use ooxml_core::RawNode;
use ooxml_pptx::{Presentation, ShapeKind, TextBodyRef, TextItem};
use serde_json::{Map, Value, json};

/// Attributes as a JSON object (Clark notation for namespaced names);
/// `null` when the element is absent or has no attributes.
pub fn attrs(n: Option<&RawNode>) -> Value {
    let Some(n) = n else { return Value::Null };
    let mut m = Map::new();
    for a in n.attrs() {
        if a.name().is_namespace_declaration() {
            continue;
        }
        let k = if a.name().ns().is_empty() {
            a.name().local().to_owned()
        } else {
            format!("{{{}}}{}", a.name().ns(), a.name().local())
        };
        m.insert(k, Value::String(a.value().to_owned()));
    }
    if m.is_empty() { Value::Null } else { Value::Object(m) }
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
                    TextItem::Run(r) => Some(json!(["r", r.text(), attrs(child(r.raw(), "rPr"))])),
                    TextItem::Break => Some(json!(["br"])),
                    TextItem::Field(f) => Some(json!(["fld", f.text()])),
                    _ => None,
                })
                .collect();
            json!({
                "ppr": attrs(child(p.raw(), "pPr")),
                "end": attrs(child(p.raw(), "endParaRPr")),
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
        let info = slide.info();
        slides.push(json!({"slide_id": info.slide_id, "partname": format!("/{}", info.part_name), "shapes": shapes}));
    }
    json!({ "slides": slides })
}
