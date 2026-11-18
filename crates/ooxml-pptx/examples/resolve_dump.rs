//! Writes ooxml-pptx's resolved placeholder geometry and links for every
//! fixture as JSON, for the independent python-pptx check (ADR-0008 §7 O1,
//! O3):
//!
//! ```text
//! cargo run -p ooxml-pptx --example resolve_dump -- OUT.json
//! python3 -I crates/ooxml-pptx/tests/oracle/resolve_check.py OUT.json reference/fixtures/pptx
//! ```

#[path = "../tests/oracle/cases.rs"]
mod cases;

use ooxml_pptx::{LinkEvent, LinkTarget, Lookup, Presentation, ShapeKind, ShowJump, Source, TextItem};
use serde_json::{Value, json};

fn target(t: &LinkTarget) -> Value {
    match t {
        LinkTarget::Url(u) => json!({"kind": "url", "address": u}),
        LinkTarget::Part(p) => json!({"kind": "part", "address": p}),
        LinkTarget::Slide { slide_id, .. } => json!({"kind": "slide", "slide_id": slide_id}),
        LinkTarget::Jump(j) => json!({"kind": "jump", "jump": match j {
            ShowJump::First => "firstslide", ShowJump::Last => "lastslide", ShowJump::Next => "nextslide",
            ShowJump::Previous => "previousslide", ShowJump::LastViewed => "lastslideviewed", ShowJump::EndShow => "endshow" }}),
        LinkTarget::Presentation(_) => json!({"kind": "verb", "verb": "hlinkpres"}),
        LinkTarget::File(_) => json!({"kind": "verb", "verb": "hlinkfile"}),
        LinkTarget::Program(_) => json!({"kind": "verb", "verb": "program"}),
        LinkTarget::Macro(_) => json!({"kind": "verb", "verb": "macro"}),
        LinkTarget::Ole(_) => json!({"kind": "verb", "verb": "ole"}),
        LinkTarget::CustomShow { .. } => json!({"kind": "verb", "verb": "customshow"}),
        LinkTarget::Media => json!({"kind": "verb", "verb": "media"}),
        LinkTarget::NoAction => json!({"kind": "verb", "verb": "noaction"}),
        other => json!({"kind": "other", "debug": format!("{other:?}")}),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: resolve_dump OUT.json")?;
    let mut all = serde_json::Map::new();
    for f in cases::FIXTURES {
        let mut pres = Presentation::from_bytes(cases::read_fixture(f))?;
        let deck = pres.deck()?;
        let (mut geometry, mut links) = (Vec::new(), Vec::new());
        for i in 0..deck.slide_count() {
            let Ok(s) = deck.slide(i) else { continue };
            let sid = s.info().slide_id;
            for sh in s.slide().shapes() {
                if *sh.kind() == ShapeKind::AlternateContent {
                    continue;
                }
                let Ok(id) = sh.id() else { continue };
                if s.chain_parts().is_ok() && s.placeholder_match(&sh)?.is_some() {
                    let g = s.geometry(&sh)?;
                    let src = match g.source() {
                        None => "none",
                        Some(Source::Direct) => "slide",
                        Some(Source::Placeholder { role: ooxml_pptx::Role::Layout, .. }) => "layout",
                        Some(_) => "master",
                    };
                    let v = match &g {
                        Lookup::Set(r) => r.value.as_ref().ok().map(|x| json!([x.x, x.y, x.cx, x.cy])),
                        _ => None,
                    };
                    geometry.push(json!({"slide_id": sid, "shape_id": id, "xywh": v, "source": src}));
                }
                if let Some(l) = s.shape_action(&sh, LinkEvent::Click) {
                    links.push(json!({"slide_id": sid, "shape_id": id, "run": null, "target": target(&l.target)}));
                }
                if *sh.kind() == ShapeKind::Shape && let Some(b) = sh.text_body() {
                    let mut k = 0;
                    for para in b.paragraphs() {
                        for it in para.items() {
                            if let TextItem::Run(r) = it {
                                if let Some(l) = s.text_link(r.raw(), LinkEvent::Click) {
                                    links.push(json!({"slide_id": sid, "shape_id": id, "run": k, "target": target(&l.target), "action": l.action}));
                                }
                                k += 1;
                            }
                        }
                    }
                }
            }
        }
        all.insert((*f).to_owned(), json!({"geometry": geometry, "links": links}));
    }
    std::fs::write(&out, serde_json::to_string_pretty(&Value::Object(all))?)?;
    println!("{} fixtures written to {out}", cases::FIXTURES.len());
    Ok(())
}
