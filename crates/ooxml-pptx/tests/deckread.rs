//! ADR-0008 §3 read APIs on real fixtures (§7 O6) and synthetic inputs for
//! what the corpus lacks.

#[path = "oracle/cases.rs"]
mod cases;

use ooxml_core::Package;
use ooxml_pptx::{Anchor, Branch, Build, LinkColor, LinkEvent, LinkTarget, Presentation, Target};

fn open(f: &str) -> Presentation {
    Presentation::from_bytes(cases::read_fixture(f)).unwrap()
}

fn patched(f: &str, part: &str, from: &str, to: &str) -> Presentation {
    let mut pkg = Package::from_bytes(cases::read_fixture(f)).unwrap();
    let x = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    assert!(x.contains(from), "{from:?}");
    pkg.set_part(part, x.replacen(from, to, 1).into_bytes()).unwrap();
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

#[test]
fn sections() {
    let mut p = open("poi/bug65551.pptx");
    let d = p.deck().unwrap();
    let s = d.sections().unwrap();
    assert_eq!(s.sections.len(), 10);
    assert_eq!(s.sections[0].name, "Home");
    assert!(s.unsectioned.is_empty() && s.duplicated.is_empty() && s.unknown.is_empty(), "{s:?}");
    let mut p = open("libreoffice/slide-sections.pptx");
    let d = p.deck().unwrap();
    let s = d.sections().unwrap();
    assert_eq!(s.sections.len(), 2);
    let listed: usize = s.sections.iter().map(|x| x.slide_ids.len()).sum();
    assert_eq!(listed, d.slide_count());
    assert!(s.sections.iter().flat_map(|x| &x.slides).all(Option::is_some));
    assert!(open("poi/sample.pptx").deck().unwrap().sections().is_none());
}

#[test]
fn legacy_and_modern_comments() {
    let mut p = open("poi/45545_Comment.pptx");
    let d = p.deck().unwrap();
    let all: Vec<_> = (0..d.slide_count()).flat_map(|i| d.slide(i).unwrap().comments().unwrap()).collect();
    let c = all.iter().find(|c| c.text == "testdoc").expect("legacy comment");
    assert!(!c.modern);
    assert_eq!(c.pos, Some((3472, 1558)));
    assert_eq!(c.created, Some("2007-10-23T09:14:49.723"));
    assert!(c.author.is_ok(), "{:?}", c.author);
    let mut p = open("libreoffice/modern-comments.pptx");
    let d = p.deck().unwrap();
    let cs = d.slide(0).unwrap().comments().unwrap();
    assert_eq!(cs.len(), 1);
    let c = &cs[0];
    assert!(c.modern);
    assert_eq!(c.text, "Make this colorful");
    assert_eq!(c.replies.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["Nice and big", "Faster"]);
    assert_eq!(c.author.as_ref().unwrap().name, "Comment Author");
    assert_eq!(c.anchor, Anchor::Shapes(vec![2]));
    // SYNTHETIC: the anchor as a text range
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    let mut pkg = Package::from_bytes(cases::read_fixture("libreoffice/modern-comments.pptx")).unwrap();
    let x = String::from_utf8(pkg.read_part(PART).unwrap()).unwrap();
    let y = x.replacen("<ac:deMkLst ", "<ac:txMkLst ", 1).replacen("</ac:deMkLst>", "<ac:txMk cp=\"3\" len=\"5\"/></ac:txMkLst>", 1);
    pkg.set_part(PART, y.into_bytes()).unwrap();
    let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let d = p.deck().unwrap();
    let c = &d.slide(0).unwrap().comments().unwrap()[0];
    assert_eq!(c.anchor, Anchor::Text { shape_id: Some(2), cp: Some(3), len: Some(5) });
}

#[test]
fn modern_shape_comment_survives_title_edit() {
    for fixture in ["libreoffice/modern-comments.pptx", "project/powerpoint-shape-comment.pptx"] {
        let mut p = open(fixture);
        let d = p.deck().unwrap();
        let comments = d.slide(0).unwrap().comments().unwrap();
        assert_eq!(comments.len(), 1, "{fixture}");
        assert!(comments[0].modern, "{fixture}");
        assert_eq!(comments[0].anchor, Anchor::Shapes(vec![2]), "{fixture}");

        p.slide_mut(0).unwrap().shape_mut_by_id(2).unwrap().text_body_mut().unwrap().set_text("Hello, edited").unwrap();
        let mut reopened = Presentation::from_bytes(p.to_bytes().unwrap()).unwrap();
        if fixture == "project/powerpoint-shape-comment.pptx" {
            let part = "ppt/comments/modernComment_100_BDA71F5F.xml";
            let original = Package::from_bytes(cases::read_fixture(fixture)).unwrap();
            assert_eq!(
                reopened.package().read_part(part).unwrap(),
                original.read_part(part).unwrap(),
                "{fixture}"
            );
        }
        let slide = reopened.slide(0).unwrap();
        assert_eq!(slide.shape_by_id(2).unwrap().unwrap().text_body().unwrap().text(), "Hello, edited", "{fixture}");
        let d = reopened.deck().unwrap();
        let comments = d.slide(0).unwrap().comments().unwrap();
        assert_eq!(comments.len(), 1, "{fixture}");
        assert!(comments[0].modern, "{fixture}");
        assert_eq!(comments[0].anchor, Anchor::Shapes(vec![2]), "{fixture}");
    }
}

#[test]
fn transitions_choose_branches_by_uri() {
    let mut p = open("libreoffice/airplane-transition.pptx");
    let d = p.deck().unwrap();
    let s = d.slide(0).unwrap();
    let t = s.transition().unwrap();
    assert_eq!(t.branch, Branch::Choice { requires: "p15" });
    assert_eq!(t.kind.map(|k| k.1), Some("prstTrans"));
    assert_eq!(t.kind_attributes, [(("", "prst"), "airplane")]);
    assert_eq!((t.speed, t.duration_ms, t.advance_on_click), (ooxml_core::EnumValue::Known(ooxml_schema::pml::TransitionSpeedValues::Slow), Some(1250), true));
    assert_eq!(s.transition_branches().len(), 2);
    // SYNTHETIC: the Requires prefix bound to an unknown URI → Fallback
    let mut p = patched("libreoffice/airplane-transition.pptx", "ppt/slides/slide2.xml", "xmlns:p15=\"http://schemas.microsoft.com/office/powerpoint/2012/main\" Requires", "xmlns:p15=\"urn:unknown\" Requires");
    let d = p.deck().unwrap();
    let t = d.slide(0).unwrap().transition().unwrap();
    assert_eq!(t.branch, Branch::Fallback);
    assert_eq!(t.kind.map(|k| k.1), Some("fade"));
    // SYNTHETIC: an empty Requires is not understood (#56 nit) → Fallback
    let mut p = patched("libreoffice/airplane-transition.pptx", "ppt/slides/slide2.xml", "Requires=\"p15\"", "Requires=\"\"");
    let d = p.deck().unwrap();
    assert_eq!(d.slide(0).unwrap().transition().unwrap().branch, Branch::Fallback);
    // SYNTHETIC: p159 (morph) is understood (Q5)
    let mut p = patched(
        "libreoffice/airplane-transition.pptx",
        "ppt/slides/slide2.xml",
        "<mc:Choice xmlns:p15=\"http://schemas.microsoft.com/office/powerpoint/2012/main\" Requires=\"p15\"><p:transition spd=\"slow\" xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\" p14:dur=\"1250\"><p15:prstTrans prst=\"airplane\"/></p:transition>",
        "<mc:Choice xmlns:p159=\"http://schemas.microsoft.com/office/powerpoint/2015/09/main\" Requires=\"p159\"><p:transition spd=\"slow\"><p159:morph option=\"byObject\"/></p:transition>",
    );
    let d = p.deck().unwrap();
    let t = d.slide(0).unwrap().transition().unwrap();
    assert_eq!(t.kind.map(|k| k.1), Some("morph"));
    // a plain direct transition
    let mut p = open("poi/bug68703.pptx");
    let d = p.deck().unwrap();
    assert!((0..d.slide_count()).any(|i| d.slide(i).unwrap().transition().is_some()));
}

#[test]
fn timing_summary() {
    let mut p = open("poi/45545_Comment.pptx");
    let d = p.deck().unwrap();
    let t = (0..d.slide_count()).find_map(|i| d.slide(i).unwrap().timing()).expect("timing");
    assert!(t.builds.iter().any(|b| matches!(b, Build::Paragraph { spid: 584707, build, .. } if build.as_str() == "p")), "{:?}", t.builds);
    assert!(t.effects.iter().any(|e| e.targets.iter().any(|x| matches!(x, Target::Paragraphs(584707, ..)))));
    assert!(t.effects.iter().all(|e| e.preset_class.is_some()));
}

#[test]
fn links() {
    // run links (with_japanese) and slide jumps (shapes.pptx table cells)
    let mut p = open("poi/with_japanese.pptx");
    let d = p.deck().unwrap();
    let ls = d.slide(0).unwrap().links();
    assert!(ls.iter().any(|(_, l)| matches!(&l.target, LinkTarget::Url(u) if u.starts_with("http"))), "{ls:?}");
    assert!(ls.iter().all(|(_, l)| l.event != LinkEvent::Click || l.color_rule() == Some(LinkColor::ThemeHyperlink)));
    let mut p = open("poi/shapes.pptx");
    let d = p.deck().unwrap();
    let ls = d.slide(4).unwrap().links();
    assert!(ls.iter().any(|(loc, l)| loc.cell.is_some() && matches!(l.target, LinkTarget::Slide { .. })), "{ls:?}");
    // SYNTHETIC: ahyp:hlinkClr val="tx" → the run's colour; a mouse-over link has no colour rule
    let mut p = patched(
        "poi/sample.pptx",
        "ppt/slides/slide1.xml",
        "<a:rPr lang=\"en-US\"><a:sym",
        "<a:rPr lang=\"en-US\"><a:hlinkClick r:id=\"\" action=\"ppaction://hlinkshowjump?jump=nextslide\"><a:extLst><a:ext uri=\"{A12FA001-AC4F-418D-AE19-62706E023703}\"><ahyp:hlinkClr xmlns:ahyp=\"http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor\" val=\"tx\"/></a:ext></a:extLst></a:hlinkClick><a:hlinkMouseOver r:id=\"\" action=\"ppaction://noaction\"/><a:sym",
    );
    let d = p.deck().unwrap();
    let ls = d.slide(0).unwrap().links();
    let click = ls.iter().find(|(_, l)| l.event == LinkEvent::Click).unwrap();
    assert_eq!(click.1.target, LinkTarget::Jump(ooxml_pptx::ShowJump::Next));
    assert_eq!(click.1.color_rule(), Some(LinkColor::RunColor));
    let hover = ls.iter().find(|(_, l)| l.event == LinkEvent::Hover).unwrap();
    assert_eq!((hover.1.target.clone(), hover.1.color_rule()), (LinkTarget::NoAction, None));
}

/// Survivor Y17: every click group of the main sequence gets its ordinal.
#[test]
fn click_index_counts_click_groups() {
    let mut checked = 0;
    for f in cases::FIXTURES {
    let bytes = cases::read_fixture(f);
    let pkg = Package::from_bytes(bytes.clone()).unwrap();
    let mut p = Presentation::from_bytes(bytes).unwrap();
    let d = p.deck().unwrap();
    for i in 0..d.slide_count() {
        let Ok(s) = d.slide(i) else { continue };
        let Some(t) = s.timing() else { continue };
        let xml = String::from_utf8(pkg.read_part(&s.info().part_name).unwrap()).unwrap();
        let main = &xml[xml.find(r#"nodeType="mainSeq""#).unwrap_or(xml.len())..];
        let clicks = main.matches(r#"nodeType="clickEffect""#).count();
        let mut seen: Vec<usize> = t.effects.iter().filter_map(|e| e.click_index).collect();
        seen.dedup();
        if clicks > 0 {
            assert_eq!(seen, (0..clicks).collect::<Vec<_>>(), "{f} slide {i}");
            checked += 1;
        }
    }
    }
    assert!(checked > 0);
}

/// Review #56 m4: timing inside mc:AlternateContent follows the
/// transition rule (Requires by namespace URI); an AC-wrapped shape's
/// action is read from its representative.
#[test]
fn alternate_content_timing_and_shape_actions() {
    let timing = |class: &str| format!(r#"<p:timing><p:tnLst><p:par><p:cTn id="1" presetClass="{class}" presetID="1"/></p:par></p:tnLst></p:timing>"#);
    let ac = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:zz="urn:unknown" Requires="zz">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent></p:sld>"#,
        timing("exit"),
        timing("entr")
    );
    let sp = r#"<p:sp><p:nvSpPr><p:cNvPr id="70" name="A"><a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/></p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>"#;
    let shape_ac = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" Requires="p14">{sp}</mc:Choice><mc:Fallback>{sp}</mc:Fallback></mc:AlternateContent></p:spTree>"#
    );
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let x = String::from_utf8(pkg.read_part("ppt/slides/slide1.xml").unwrap()).unwrap();
    assert!(!x.contains("<p:timing"));
    let x = x.replacen("</p:sld>", &ac, 1).replacen("</p:spTree>", &shape_ac, 1);
    pkg.set_part("ppt/slides/slide1.xml", x.into_bytes()).unwrap();
    let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let d = p.deck().unwrap();
    let s = d.slide(0).unwrap();
    let t = s.timing().unwrap();
    assert_eq!(t.effects.iter().map(|e| e.preset_class.map(|c| c.as_str())).collect::<Vec<_>>(), [Some("entr")]);
    let shapes = s.slide().shapes();
    let acs = shapes.iter().find(|x| *x.kind() == ooxml_pptx::ShapeKind::AlternateContent).unwrap();
    let l = s.shape_action(acs, LinkEvent::Click).expect("AC shape action");
    assert_eq!(l.target, LinkTarget::Jump(ooxml_pptx::ShowJump::Next));
    assert!(s.links().iter().any(|(loc, _)| loc.shape == acs.path()));
}

/// Review #56 m5: reading and locking classify anchors with one function:
/// a `pc:` anchor other than `pc:sldMkLst` without ids is not a slide anchor.
#[test]
fn comment_anchor_shared_classifier() {
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    let mut pkg = Package::from_bytes(cases::read_fixture("libreoffice/modern-comments.pptx")).unwrap();
    let x = String::from_utf8(pkg.read_part(PART).unwrap()).unwrap();
    let start = x.find("<ac:deMkLst").unwrap();
    let end = x.find("</ac:deMkLst>").unwrap() + "</ac:deMkLst>".len();
    let y = format!("{}<pc:sldLayoutMkLst xmlns:pc=\"http://schemas.microsoft.com/office/powerpoint/2013/main/command\"><pc:docMk/></pc:sldLayoutMkLst>{}", &x[..start], &x[end..]);
    pkg.set_part(PART, y.into_bytes()).unwrap();
    let bytes = pkg.to_bytes().unwrap();
    let mut p = Presentation::from_bytes(bytes).unwrap();
    {
        let d = p.deck().unwrap();
        let c = &d.slide(0).unwrap().comments().unwrap()[0];
        assert_eq!(c.anchor, Anchor::Other("sldLayoutMkLst".into()));
    }
    // ... and the lock side locks every shape's text
    let id = p.slide(0).unwrap().shapes().into_iter().find_map(|s| s.text_body().and(s.id().ok())).unwrap();
    let e = p.slide_mut(0).unwrap().shape_mut_by_id(id).unwrap().text_body_mut().unwrap().set_text("x").unwrap_err();
    assert!(matches!(e, ooxml_pptx::Error::CommentAnchoredText { .. }), "{e:?}");
}

/// Review #56 r2-m2 (Z1, Z2): effects before the first click and in an
/// interactive sequence have no click index; the comparison keeps `None`s.
#[test]
fn click_index_exact_sequence() {
    let eff = |id: u32, nt: &str| format!(r#"<p:par><p:cTn id="{id}" presetClass="entr" presetID="1" nodeType="{nt}"/></p:par>"#);
    let main: String = [eff(3, "afterEffect"), eff(4, "clickEffect"), eff(5, "withEffect"), eff(6, "clickEffect")].concat();
    let timing = format!(
        r#"<p:timing><p:tnLst><p:par><p:cTn id="1" nodeType="tmRoot"><p:childTnLst><p:seq><p:cTn id="2" nodeType="mainSeq"><p:childTnLst>{main}</p:childTnLst></p:cTn></p:seq><p:seq><p:cTn id="7" nodeType="interactiveSeq"><p:childTnLst>{}</p:childTnLst></p:cTn></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>"#,
        eff(8, "clickEffect")
    );
    let mut p = patched("poi/sample.pptx", "ppt/slides/slide1.xml", "</p:sld>", &timing);
    let d = p.deck().unwrap();
    let t = d.slide(0).unwrap().timing().unwrap();
    assert_eq!(t.effects.iter().map(|e| e.click_index).collect::<Vec<_>>(), [None, Some(0), Some(0), Some(1), None]);
}

/// Z22: transition kind attributes keep their namespace.
#[test]
fn transition_kind_attribute_namespaces() {
    let mut p = patched("libreoffice/airplane-transition.pptx", "ppt/slides/slide2.xml", r#"<p15:prstTrans prst="airplane"/>"#, r#"<p15:prstTrans prst="airplane" p14:dir="r"/>"#);
    let d = p.deck().unwrap();
    let t = d.slide(0).unwrap().transition().unwrap();
    assert_eq!(t.kind_attributes, [(("", "prst"), "airplane"), (("http://schemas.microsoft.com/office/powerpoint/2010/main", "dir"), "r")]);
}
