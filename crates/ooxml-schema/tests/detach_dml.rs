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

// ---- same-part copies (ADR-0002 note "same-part copy") ----

const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The ADR-0016 case: splitting a run whose `a:rPr` holds a hyperlink. The
/// copy keeps `r:id` (same part, same `.rels`) and, when allowed, the
/// `ahyp:hlinkClr` extension; `smtId` and effect container names still refuse.
#[test]
fn hyperlink_run_properties_copy_within_the_part() {
    use ooxml_core::SamePartOptions;
    use ooxml_core::lens::ext_uri::HYPERLINK_COLOR;
    use ooxml_schema::dml::{Paragraph, Run};
    let inner = format!(
        concat!(
            r#"<a:r><a:rPr lang="en-US" dirty="0"><a:hlinkClick r:id="rId2" tooltip="t">"#,
            r#"<a:extLst><a:ext uri="{u}"><ahyp:hlinkClr xmlns:ahyp="http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor" val="tx"/></a:ext></a:extLst>"#,
            r#"</a:hlinkClick></a:rPr><a:t>link</a:t></a:r><a:r><a:t>plain</a:t></a:r>"#
        ),
        u = HYPERLINK_COLOR
    );
    let src = format!(r#"<a:p {A} xmlns:r="{R_NS}">{inner}</a:p>"#);
    let mut d = XmlDocument::parse(src.as_bytes()).unwrap();
    let rpr = d.lens_at::<RunProperties>(&[0, 0]).unwrap();
    assert_matches!(rpr.detach_as::<RunProperties>(&[]), Err(Error::NotDetachable { .. }), "any-part copy refuses links");
    assert_matches!(
        rpr.detach_same_part_as::<RunProperties>(&[], &SamePartOptions::default()),
        Err(Error::NotDetachable { reason: DetachRefusal::ExtensionList, .. }),
        "the extension needs an explicit opt-in"
    );
    let copy = rpr.detach_same_part_as::<RunProperties>(&[], &SamePartOptions::default().keep_extension(HYPERLINK_COLOR)).unwrap();
    let mut p = d.lens_mut::<Paragraph>().unwrap();
    p.nth_child_mut::<Run>(1).unwrap().insert_same_part(&copy).unwrap();
    let out = d.serialize();
    assert_eq!(out.matches(r#"<a:hlinkClick r:id="rId2" tooltip="t">"#).count(), 2, "{out}");
    assert_eq!(out.matches("ahyp:hlinkClr").count(), 2, "{out}");
    // also as the paragraph's end properties (a:endParaRPr, SameType)
    let end = d.lens_at::<RunProperties>(&[0, 0]).unwrap();
    let end = end.detach_same_part_as::<EndParagraphRunProperties>(&[], &SamePartOptions::default().keep_extension(HYPERLINK_COLOR)).unwrap();
    d.lens_mut::<Paragraph>().unwrap().insert_same_part(&end).unwrap();
    XmlDocument::parse(d.serialize().as_bytes()).unwrap();
    // smtId and effect container names still refuse in a same-part copy
    for bad in [r#"<a:rPr smtId="5"><a:hlinkClick r:id="rId2"/></a:rPr>"#, r#"<a:rPr><a:effectDag name="E"/><a:hlinkClick r:id="rId2"/></a:rPr>"#] {
        let d = XmlDocument::parse(format!(r#"<a:p {A} xmlns:r="{R_NS}"><a:r>{bad}<a:t>x</a:t></a:r></a:p>"#).as_bytes()).unwrap();
        let rpr = d.lens_at::<RunProperties>(&[0, 0]).unwrap();
        assert_matches!(
            rpr.detach_same_part_as::<RunProperties>(&[], &SamePartOptions::default()),
            Err(Error::NotDetachable { reason: DetachRefusal::UniqueAttribute, .. }),
            "{bad}"
        );
    }
}

/// Corpus run (ignored; needs the files): every slide `a:rPr` that holds a
/// relationship id — 83 in the 619-presentation ADR-0008 corpus, all
/// hyperlinks, 12 with `ahyp:hlinkClr` (each declaring `xmlns:ahyp` on
/// itself) — is copied into a new run of its own paragraph with a same-part
/// copy. The source bytes appear once more in the output (verbatim), it
/// re-parses, and the copy is structurally equal to its source.
///
/// `OOXML_SAME_PART_CORPUS` names a file listing `.pptx` paths, one per line.
#[test]
#[ignore = "needs a local pptx corpus: OOXML_SAME_PART_CORPUS=<file listing .pptx paths>"]
fn corpus_hyperlink_runs_copy_within_their_slide() {
    use ooxml_core::lens::ext_uri::HYPERLINK_COLOR;
    use ooxml_core::{Package, RawNode, SamePartOptions};
    use ooxml_schema::dml::{Paragraph, Run};
    let list = std::env::var("OOXML_SAME_PART_CORPUS").expect("OOXML_SAME_PART_CORPUS");
    let paths: Vec<String> = std::fs::read_to_string(list).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
    fn has_rel(n: &RawNode) -> bool {
        n.attrs().iter().any(|a| a.name().ns() == R_NS) || n.elements().any(has_rel)
    }
    fn find(n: &RawNode, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, c) in n.indexed_elements() {
            path.push(i);
            if c.is(<RunProperties as ooxml_core::ElementSpec>::QNAME) && n.is(<Run as ooxml_core::ElementSpec>::QNAME) && has_rel(c) {
                out.push(path.clone());
            }
            find(c, path, out);
            path.pop();
        }
    }
    let opts = SamePartOptions::default().keep_extension(HYPERLINK_COLOR);
    let (mut copied, mut with_ext) = (0, 0);
    for path in &paths {
        let mut pkg = Package::from_bytes(std::fs::read(path).unwrap()).unwrap();
        let slides: Vec<String> = pkg.part_names().filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")).map(str::to_owned).collect();
        for slide in slides {
            let base = pkg.xml_part(&slide).unwrap().clone();
            let mut targets = Vec::new();
            find(base.root(), &mut Vec::new(), &mut targets);
            for t in targets {
                let mut d = XmlDocument::parse(base.source().as_bytes()).unwrap();
                let src = d.lens_at::<RunProperties>(&t).unwrap();
                with_ext += usize::from(src.raw().elements().any(|e| e.elements().any(|x| x.name().local() == "extLst")));
                let copy = src.detach_same_part_as::<RunProperties>(&[], &opts).unwrap_or_else(|e| panic!("{path} {slide} {t:?}: {e}"));
                let para = &t[..t.len() - 2];
                let mut p = d.lens_mut_at::<Paragraph>(para).unwrap();
                let mut run = p.insert::<Run>().unwrap_or_else(|e| panic!("{path} {slide} {t:?}: {e}"));
                run.insert_same_part(&copy).unwrap_or_else(|e| panic!("{path} {slide} {t:?}: {e}"));
                let new_run = run.path().to_vec();
                let out = d.serialize();
                // byte for byte: the source rPr's text appears exactly once more
                let sp = base.lens_at::<RunProperties>(&t).unwrap().raw().span().expect("source span");
                let text = &base.source()[sp.start..sp.end()];
                assert_eq!(out.matches(text).count(), base.source().matches(text).count() + 1, "{path} {slide} {t:?}");
                let again = XmlDocument::parse(out.as_bytes()).unwrap();
                let a = again.lens_at::<RunProperties>(&t).unwrap();
                let mut new_rpr = new_run.clone();
                new_rpr.push(0);
                let b = again.lens_at::<RunProperties>(&new_rpr).unwrap();
                assert!(a.raw().structurally_eq(b.raw()), "{path} {slide} {t:?}");
                copied += 1;
            }
        }
    }
    println!("same-part copies: {copied} (with an extension: {with_ext})");
    assert!(copied > 0);
}
