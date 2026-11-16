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
fn transitions_choose_branches_by_uri() {
    let mut p = open("libreoffice/airplane-transition.pptx");
    let d = p.deck().unwrap();
    let s = d.slide(0).unwrap();
    let t = s.transition().unwrap();
    assert_eq!(t.branch, Branch::Choice { requires: "p15" });
    assert_eq!(t.kind.map(|k| k.1), Some("prstTrans"));
    assert_eq!(t.kind_attributes, [("prst", "airplane")]);
    assert_eq!((t.speed, t.duration_ms, t.advance_on_click), ("slow", Some(1250), true));
    assert_eq!(s.transition_branches().len(), 2);
    // SYNTHETIC: the Requires prefix bound to an unknown URI → Fallback
    let mut p = patched("libreoffice/airplane-transition.pptx", "ppt/slides/slide2.xml", "xmlns:p15=\"http://schemas.microsoft.com/office/powerpoint/2012/main\" Requires", "xmlns:p15=\"urn:unknown\" Requires");
    let d = p.deck().unwrap();
    let t = d.slide(0).unwrap().transition().unwrap();
    assert_eq!(t.branch, Branch::Fallback);
    assert_eq!(t.kind.map(|k| k.1), Some("fade"));
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
    assert!(t.builds.iter().any(|b| matches!(b, Build::Paragraph { spid: 584707, build: "p", .. })), "{:?}", t.builds);
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
