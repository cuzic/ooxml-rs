//! `XmlDocument::parse` → serialize (T1) and parse → `force_rerender` →
//! reparse (T1b).

use ooxml_core::{LeafStep, ParseOptions, RawItem, RawNode, XmlDocument};

use crate::input::Input;

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

/// Leaf-text preflight and write must agree. Refusals preserve the source
/// bytes, and successful writes survive serialization and reparsing.
pub fn check_leaf_text(data: &[u8]) {
    let mut input = Input::new(data);
    let value = input.string();
    let control_count = input.u8();
    let controls = input.take(usize::from(control_count));
    let source = input.rest();
    let (mut doc, source) = match XmlDocument::parse(source) {
        Ok(doc) => (doc, source),
        Err(_) => {
            // Keep exercising writes when the fuzzer has no well-formed XML yet.
            let fallback = b"<r xmlns:i='http://www.w3.org/2001/XMLSchema-instance'>\
                <a/><a>old</a><a i:nil='yes'/><b><!-- keep --></b><c><a/></c></r>";
            (XmlDocument::parse(fallback).unwrap(), fallback.as_slice())
        }
    };
    let mut controls = Input::new(controls);
    let mode = controls.u8() % 8;
    let mut node = doc.root();
    let mut names = vec![(node.name().clone(), 0)];
    while let Some(choice) = controls.take(1).first() {
        let count = node.elements().count();
        if count == 0 {
            break;
        }
        let position = usize::from(*choice) % count;
        let child = node.elements().nth(position).unwrap();
        let index = node
            .elements()
            .take(position)
            .filter(|sibling| sibling.name().ns() == child.name().ns() && sibling.name().local() == child.name().local())
            .count();
        names.push((child.name().clone(), index));
        node = child;
    }
    let mut path: Vec<_> = names
        .iter()
        .map(|(name, index)| LeafStep { ns: name.ns(), local: name.local(), index: *index })
        .collect();
    match mode {
        0 => path.clear(),
        1 => path.last_mut().unwrap().index = usize::MAX,
        _ => {}
    }

    let checked = doc.check_set_leaf_text(&path, &value);
    let written = doc.set_leaf_text(&path, &value);
    let after = doc.serialize();
    match (checked, written) {
        (Err(checked), Err(written)) => {
            assert_eq!(std::mem::discriminant(&checked), std::mem::discriminant(&written));
            assert_eq!(checked.to_string(), written.to_string());
            assert_eq!(after.as_bytes(), source);
        }
        (Ok(()), Ok(())) => {
            let reparsed = XmlDocument::parse(after.as_bytes()).expect("leaf-text output must reparse");
            let mut target = reparsed.root();
            let root = path.first().expect("successful write must have a root step");
            assert_eq!((target.name().ns(), target.name().local(), 0), (root.ns, root.local, root.index));
            for step in &path[1..] {
                target = target
                    .elements()
                    .filter(|child| child.name().ns() == step.ns && child.name().local() == step.local)
                    .nth(step.index)
                    .expect("leaf-text target must survive reparsing");
            }
            assert_eq!(target.text(), value);
        }
        results => panic!("leaf-text preflight and write disagree: {results:?}"),
    }
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
