//! The generated `Detachable` types of `dml` (ADR-0002 note
//! "detach/insert"): DrawingML formatting, with `smtId` (ECMA-376 Part 1
//! §21.1.2.3.9, "unique throughout the presentation") refused when set.
#![cfg(feature = "dml")]

use std::assert_matches;

use ooxml_core::{DetachRefusal, Detachable, Error, Lens, XmlDocument};
use ooxml_schema::dml::{EndParagraphRunProperties, ParagraphProperties, RunProperties};

const A: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

fn para(inner: &str) -> XmlDocument {
    XmlDocument::parse(format!("<a:p {A}>{inner}</a:p>").as_bytes()).unwrap()
}

#[test]
fn smt_id_is_a_generated_unique_attribute() {
    for list in [RunProperties::UNIQUE_ATTRS, EndParagraphRunProperties::UNIQUE_ATTRS, ParagraphProperties::UNIQUE_ATTRS] {
        let names: Vec<_> = list.iter().map(|u| (u.element.local, u.attr.local, u.default)).collect();
        for el in ["rPr", "endParaRPr", "defRPr"] {
            assert!(names.contains(&(el, "smtId", Some("0"))), "{el}: {names:?}");
        }
    }
}

#[test]
fn smt_id_and_ext_lst_are_refused_on_real_types() {
    // a:rPr with a smart-tag id
    let d = para(r#"<a:r><a:rPr lang="en-US" smtId="1"/><a:t>CNTS</a:t></a:r>"#);
    let rpr = Lens::<RunProperties>::at(d.root(), &[0, 0]).unwrap();
    assert_matches!(rpr.detach_as::<EndParagraphRunProperties>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::UniqueAttribute, .. }));
    // smtId="0" is the default, not an id
    let d = para(r#"<a:r><a:rPr lang="en-US" smtId="0"/><a:t>x</a:t></a:r>"#);
    Lens::<RunProperties>::at(d.root(), &[0, 0]).unwrap().detach_as::<RunProperties>(&[]).unwrap();
    // a:pPr whose a:defRPr carries one
    let d = para(r#"<a:pPr algn="ctr"><a:defRPr smtId="3"/></a:pPr>"#);
    let ppr = Lens::<ParagraphProperties>::at(d.root(), &[0]).unwrap();
    assert_matches!(ppr.detach_as::<ParagraphProperties>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::UniqueAttribute, .. }));
    // a:rPr with an extension list
    let d = para(r#"<a:r><a:rPr lang="en-US"><a:extLst><a:ext uri="{X}"/></a:extLst></a:rPr><a:t>x</a:t></a:r>"#);
    let rpr = Lens::<RunProperties>::at(d.root(), &[0, 0]).unwrap();
    assert_matches!(rpr.detach_as::<RunProperties>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::ExtensionList, .. }));
}

#[test]
fn effect_container_names_are_refused() {
    // ECMA-376 Part 1 §20.1.8.20 / §20.1.8.25: an effect container's name
    // "Shall be unique across all effect trees and effect containers" (PR #18
    // review round 3, R3-1). Reached through EG_EffectProperties.
    let names: Vec<_> = RunProperties::UNIQUE_ATTRS.iter().map(|u| (u.element.local, u.attr.local, u.default)).collect();
    assert!(names.contains(&("effectDag", "name", None)) && names.contains(&("cont", "name", None)), "{names:?}");
    for inner in [
        r#"<a:r><a:rPr lang="en-US"><a:effectDag name="E2" type="sib"/></a:rPr><a:t>x</a:t></a:r>"#,
        r#"<a:r><a:rPr lang="en-US"><a:effectDag type="sib"><a:cont name="C2"/></a:effectDag></a:rPr><a:t>x</a:t></a:r>"#,
    ] {
        let d = para(inner);
        let rpr = Lens::<RunProperties>::at(d.root(), &[0, 0]).unwrap();
        assert_matches!(rpr.detach_as::<EndParagraphRunProperties>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::UniqueAttribute, .. }), "{inner}");
    }
    let d = para(r#"<a:pPr><a:defRPr><a:effectDag name="E3"/></a:defRPr></a:pPr>"#);
    let ppr = Lens::<ParagraphProperties>::at(d.root(), &[0]).unwrap();
    assert_matches!(ppr.detach_as::<ParagraphProperties>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::UniqueAttribute, .. }));
    // an unnamed container is copied
    let d = para(r#"<a:r><a:rPr lang="en-US"><a:effectDag type="sib"><a:cont/></a:effectDag></a:rPr><a:t>x</a:t></a:r>"#);
    Lens::<RunProperties>::at(d.root(), &[0, 0]).unwrap().detach_as::<RunProperties>(&[]).unwrap();
}
