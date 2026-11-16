//! `XmlDocument::parse` → serialize (T1) and parse → `force_rerender` →
//! reparse (T1b).

use ooxml_core::{ParseOptions, RawItem, RawNode, XmlDocument};

/// T1: an accepted input serializes unedited to exactly its own bytes.
/// Also exercises `Clone`, `Debug`, `Drop` and the read accessors on the
/// tree, and checks that a lower depth limit never accepts what the
/// default refuses.
pub fn check_t1(data: &[u8]) {
    let Ok(doc) = XmlDocument::parse(data) else {
        // Lowering the limit can only refuse more.
        if let Ok(low) = XmlDocument::parse_with(data, &ParseOptions::default().with_max_depth(8)) {
            panic!("accepted with max_depth 8 but refused with the default: {:?}", low.root().name());
        }
        return;
    };
    assert!(doc.serialize().as_bytes() == data, "T1: unedited serialize differs from the input");
    assert!(!doc.is_dirty());
    let copy = doc.clone();
    let _ = format!("{copy:?}");
    walk(doc.root());
}

fn walk(n: &RawNode) {
    let _ = n.text();
    let _ = n.xml_space_preserve();
    for c in n.children() {
        match c {
            RawItem::Element(e) => walk(e),
            RawItem::Text(t) => {
                let _ = t.is_whitespace();
            }
            RawItem::Markup(m) => {
                let _ = m.raw();
            }
        }
    }
}

/// T1b: re-rendering the whole tree gives text that parses, with the same
/// options, into a structurally equal tree, and re-rendering that again is
/// a fixed point.
pub fn check_t1b(data: &[u8]) {
    let Ok(doc) = XmlDocument::parse(data) else { return };
    let mut r = doc.clone();
    r.force_rerender();
    let s = r.serialize();
    let re = XmlDocument::parse(s.as_bytes()).unwrap_or_else(|e| panic!("T1b: re-rendered text does not parse: {e}\n{s:?}"));
    assert!(re.root().structurally_eq(doc.root()), "T1b: re-rendered tree differs\ninput: {:?}\noutput: {s:?}", String::from_utf8_lossy(data));
    let mut rr = re.clone();
    rr.force_rerender();
    assert_eq!(rr.serialize(), s, "T1b: re-rendering is not a fixed point");
}

/// Structural equality with adjacent character data merged (an edit can
/// leave two text items side by side that a reparse reads as one).
pub fn canon_eq(a: &RawNode, b: &RawNode) -> bool {
    a.name() == b.name() && a.attrs() == b.attrs() && canon_children(a) == canon_children(b) && {
        let ea: Vec<&RawNode> = a.elements().collect();
        let eb: Vec<&RawNode> = b.elements().collect();
        ea.len() == eb.len() && ea.iter().zip(&eb).all(|(x, y)| canon_eq(x, y))
    }
}

#[derive(PartialEq, Debug)]
enum Canon<'a> {
    Element,
    Text(String),
    CData(&'a str),
    Markup(&'a str),
}

fn canon_children(n: &RawNode) -> Vec<Canon<'_>> {
    let mut out: Vec<Canon<'_>> = Vec::new();
    for c in n.children() {
        match c {
            RawItem::Element(_) => out.push(Canon::Element),
            RawItem::Text(t) if t.is_cdata() => out.push(Canon::CData(t.value())),
            RawItem::Text(t) => {
                if t.value().is_empty() {
                    continue;
                }
                if let Some(Canon::Text(prev)) = out.last_mut() {
                    prev.push_str(t.value());
                } else {
                    out.push(Canon::Text(t.value().to_owned()));
                }
            }
            RawItem::Markup(m) => out.push(Canon::Markup(m.raw())),
        }
    }
    out
}
