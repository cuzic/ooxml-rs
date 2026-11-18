//! ADR-0008 §2 (notes: read and edit existing notes slides), §1.7 (notes,
//! layouts and masters as resolution contexts) and the notes scopes of
//! find/replace (§4), §7 O4. T2 on notes bodies is in `locality.rs`, T3 in
//! `oracle.rs` / `oracle/check.py`.

#[path = "oracle/cases.rs"]
mod cases;

use std::assert_matches;

use ooxml_core::Package;
use ooxml_pptx::{
    Error, FindOptions, Lookup, MapSource, MatchKind, Presentation, ReplaceOptions, Role, RunItem, Scope, SheetKind, Source, TextItem,
    TextStyleKind,
};

const NOTES1: &str = "ppt/notesSlides/notesSlide1.xml";
/// The empty notes body paragraph of poi/sample.pptx slide 1.
const EMPTY_BODY: &str = "<a:bodyPr><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p>";

fn patched(f: &str, edits: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut pkg = Package::from_bytes(cases::read_fixture(f)).unwrap();
    for (part, from, to) in edits {
        let x = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
        assert!(x.contains(from), "{part}: {from:?}");
        pkg.set_part(part, x.replacen(from, to, 1).into_bytes()).unwrap();
    }
    pkg.to_bytes().unwrap()
}

/// sample.pptx with a two-paragraph notes body on slide 1 (a formatted run,
/// a plain one).
fn sample_with_notes_text() -> Vec<u8> {
    patched(
        "poi/sample.pptx",
        &[(NOTES1, EMPTY_BODY, "<a:bodyPr><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\" b=\"1\"/><a:t>Speaker</a:t></a:r><a:r><a:rPr lang=\"en-US\"/><a:t> notes</a:t></a:r></a:p><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>second line</a:t></a:r></a:p>")],
    )
}

/// A refused call leaves the package byte-identical.
fn refused<T: std::fmt::Debug>(p: &mut Presentation, call: impl FnOnce(&mut Presentation) -> Result<T, Error>) -> Error {
    let before = p.to_bytes().unwrap();
    let e = call(p).expect_err("refused");
    assert_eq!(p.to_bytes().unwrap(), before, "a refused call ({e}) changed the package");
    e
}

#[test]
fn reading_every_notes_part_is_lossless() {
    // O4 L1: read every notes slide (text, shapes, resolvers) and open every
    // notes body for editing without a change: the package is unchanged
    let mut notes_bodies = 0;
    for f in cases::FIXTURES {
        let mut p = Presentation::from_bytes(cases::read_fixture(f)).unwrap();
        let before = p.to_bytes().unwrap();
        for i in 0..p.slide_count() {
            let Ok(Some(n)) = p.notes(i) else { continue };
            let _ = (n.text(), n.shapes(), n.part_name().to_owned());
        }
        {
            let deck = p.deck().unwrap();
            for i in 0..deck.slide_count() {
                let Ok(s) = deck.slide(i) else { continue };
                let (Some(n), Some(sheet)) = (s.notes(), s.notes_sheet()) else { continue };
                assert_eq!(sheet.kind(), SheetKind::Notes);
                for sh in n.shapes() {
                    let _ = (sheet.geometry(&sh), sheet.body_props(&sh), sheet.color_map());
                    let Some(b) = sh.text_body() else { continue };
                    for para in b.paragraphs() {
                        if sheet.master().is_ok() {
                            sheet.para_props(&sh, b, para).unwrap();
                        }
                        for it in para.items() {
                            if let TextItem::Run(r) = it
                                && sheet.master().is_ok()
                            {
                                sheet.run_props(&sh, b, para, RunItem::Run(r)).unwrap();
                            }
                        }
                    }
                }
            }
            for sheet in deck.layouts().into_iter().chain(deck.masters()) {
                for sh in sheet.shapes() {
                    let _ = (sheet.geometry(&sh), sheet.placeholder_match(&sh));
                    let Some(b) = sh.text_body() else { continue };
                    for para in b.paragraphs() {
                        let _ = sheet.para_props(&sh, b, para);
                    }
                }
            }
        }
        for i in 0..p.slide_count() {
            if let Ok(mut n) = p.notes_mut(i)
                && n.body_mut().is_ok()
            {
                notes_bodies += 1;
            }
        }
        assert_eq!(p.to_bytes().unwrap(), before, "{f}: reading notes changed the package");
    }
    assert!(notes_bodies >= 30, "{notes_bodies}");
}

#[test]
fn shared_notes_are_replaced_once() {
    let opts = ReplaceOptions::default().find(FindOptions::default().scope(Scope::SlidesAndNotes));
    for (body, pattern, replacement, expected) in [
        ("<a:r><a:t>fo</a:t></a:r><a:r><a:t>o</a:t></a:r><a:r><a:t>!</a:t></a:r><a:r><a:t>tail</a:t></a:r>", "foo", "X", "X!tail"),
        ("<a:r><a:t>a-b</a:t></a:r>", "-", "\u{b}", "a\u{b}b"),
    ] {
        let paragraph = format!("<a:bodyPr/><a:lstStyle/><a:p>{body}</a:p>");
        let input = patched("poi/sample.pptx", &[
            ("ppt/slides/_rels/slide2.xml.rels", "../notesSlides/notesSlide2.xml", "../notesSlides/notesSlide1.xml"),
            (NOTES1, EMPTY_BODY, &paragraph),
        ]);
        let mut p = Presentation::from_bytes(input).unwrap();
        let report = p.replace_all(pattern, replacement, &opts).unwrap();
        let [matched] = report.replaced.as_slice() else { panic!("{report:?}") };
        assert!(report.skipped.is_empty());
        assert!(matched.location.notes);
        assert_eq!(matched.location.slide, 0);
        let mut saved = Presentation::from_bytes(p.to_bytes().unwrap()).unwrap();
        for i in [0, 1] {
            assert_eq!(saved.notes(i).unwrap().unwrap().text(), expected);
        }
    }
}

#[test]
fn deck_ignores_notes_relationship_to_slide() {
    let input = patched("poi/sample.pptx", &[
        ("ppt/slides/_rels/slide2.xml.rels", "../notesSlides/notesSlide2.xml", "../slides/slide1.xml"),
    ]);
    let mut p = Presentation::from_bytes(input.clone()).unwrap();
    let deck = p.deck().unwrap();
    let slide = deck.slide(1).unwrap();
    assert!(slide.notes().is_none());
    assert!(slide.notes_sheet().is_none());
    assert_eq!(p.to_bytes().unwrap(), input);
}

#[test]
fn first_body_placeholder_without_text_body_wins() {
    let input = patched("poi/sample.pptx", &[(NOTES1, r#"<p:ph type="sldImg"/>"#, r#"<p:ph type="body"/>"#)]);
    let mut p = Presentation::from_bytes(input).unwrap();
    assert!(p.notes(0).unwrap().unwrap().body().is_none());
    let deck = p.deck().unwrap();
    assert!(deck.slide(0).unwrap().notes().unwrap().body().is_none());
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut().map(|_| ())), Error::NoNotesBody);
    let find = FindOptions::default().scope(Scope::Notes(0));
    assert_matches!(refused(&mut p, |p| p.find("x", &find)), Error::NoNotesBody);
    let opts = ReplaceOptions::default().find(find);
    assert_matches!(refused(&mut p, |p| p.replace_all("x", "y", &opts)), Error::NoNotesBody);
}

#[test]
fn notes_text_and_editing() {
    let mut p = Presentation::from_bytes(sample_with_notes_text()).unwrap();
    let n = p.notes(0).unwrap().unwrap();
    assert_eq!(n.part_name(), NOTES1);
    assert_eq!(n.text(), "Speaker notes\nsecond line");
    assert_eq!(n.body_shape().unwrap().id().unwrap(), 3);
    let input = p.to_bytes().unwrap();
    {
        let mut nm = p.notes_mut(0).unwrap();
        let mut b = nm.body_mut().unwrap();
        b.paragraph_mut(0).unwrap().run_mut(1).unwrap().set_text(" remarks").unwrap();
        b.add_paragraph("third\u{b}line").unwrap();
        assert_eq!(nm.as_ref().text(), "Speaker remarks\nsecond line\nthird\u{b}line");
    }
    {
        let mut nm = p.notes_mut(0).unwrap();
        nm.body_mut().unwrap().set_text("one\ntwo").unwrap();
    }
    assert_eq!(p.notes(0).unwrap().unwrap().text(), "one\ntwo");
    // only the notes part changed (ADR-0008 §2)
    let out = p.to_bytes().unwrap();
    let (a, b) = (Package::from_bytes(input).unwrap(), Package::from_bytes(out.clone()).unwrap());
    let names: Vec<String> = a.part_names().map(str::to_owned).collect();
    assert_eq!(names, b.part_names().map(str::to_owned).collect::<Vec<_>>());
    for n in &names {
        if n != NOTES1 {
            assert_eq!(a.read_part(n).unwrap(), b.read_part(n).unwrap(), "{n} changed");
        }
    }
    assert_ne!(a.read_part(NOTES1).unwrap(), b.read_part(NOTES1).unwrap());
    // the first paragraph's run keeps its format (Phase 0 set_text)
    let mut q = Presentation::from_bytes(out).unwrap();
    let n = q.notes(0).unwrap().unwrap();
    let paras = n.body().unwrap().paragraphs();
    let bold = |k: usize| paras[k].runs()[0].raw().elements().next().and_then(|r| r.attr(ooxml_core::QName::new("", "b"))) == Some("1");
    assert!(bold(0) && !bold(1));
}

#[test]
fn notes_refusals() {
    // T4: NoNotesSlide on every slide without notes
    let mut checked = 0;
    for f in cases::FIXTURES {
        let mut p = Presentation::from_bytes(cases::read_fixture(f)).unwrap();
        for i in 0..p.slide_count() {
            if p.slide(i).is_ok() && p.notes(i).unwrap().is_none() {
                let id = p.slide(i).unwrap().info().slide_id;
                assert_matches!(refused(&mut p, |p| p.notes_mut(i).map(|_| ())), Error::NoNotesSlide { slide_id } if slide_id == id);
                let o = FindOptions::default().scope(Scope::Notes(i));
                assert_matches!(
                    refused(&mut p, |p| p.find("e", &o)),
                    Error::NoNotesSlide { slide_id } if slide_id == id
                );
                let opts = ReplaceOptions::default().find(o);
                assert_matches!(
                    refused(&mut p, |p| p.replace_all("e", "E", &opts)),
                    Error::NoNotesSlide { slide_id } if slide_id == id
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
    // SYNTHETIC: the notes body placeholder retyped → NoNotesBody
    let mut p = Presentation::from_bytes(patched("poi/sample.pptx", &[(NOTES1, "<p:ph type=\"body\" idx=\"1\"/>", "<p:ph type=\"obj\" idx=\"1\"/>")])).unwrap();
    assert!(p.notes(0).unwrap().unwrap().body().is_none());
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut().map(|_| ())), Error::NoNotesBody);
    let o = FindOptions::default().scope(Scope::Notes(0));
    assert_matches!(refused(&mut p, |p| p.find("x", &o)), Error::NoNotesBody);
    let opts = ReplaceOptions::default().find(o);
    assert_matches!(
        refused(&mut p, |p| p.replace_all("x", "y", &opts)),
        Error::NoNotesBody
    );
    // ... and SlidesAndNotes skips it
    assert!(p.find("Lorem", &FindOptions::default().scope(Scope::SlidesAndNotes)).unwrap().iter().all(|m| !m.location.notes));
    // SYNTHETIC: the notes body inside mc:AlternateContent → InsideAlternateContent
    let ac = patched(
        "poi/sample.pptx",
        &[
            (NOTES1, "<p:sp><p:nvSpPr><p:cNvPr id=\"3\"", "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Fallback><p:sp><p:nvSpPr><p:cNvPr id=\"3\""),
            (NOTES1, "</p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"4\"", "</p:txBody></p:sp></mc:Fallback></mc:AlternateContent><p:sp><p:nvSpPr><p:cNvPr id=\"4\""),
            (NOTES1, "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", "<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>hidden</a:t></a:r></a:p>"),
        ],
    );
    let mut p = Presentation::from_bytes(ac).unwrap();
    assert!(p.notes(0).unwrap().unwrap().body().is_none());
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut().map(|_| ())), Error::InsideAlternateContent);
    // find sees it; replace refuses it (ADR-0008 §2: the AC rule applies)
    let o = FindOptions::default().scope(Scope::Notes(0));
    assert_eq!(p.find("hidden", &o).unwrap().len(), 1);
    let before = p.to_bytes().unwrap();
    let rep = p.replace_all("hidden", "shown", &ReplaceOptions::default().find(o)).unwrap();
    assert!(rep.replaced.is_empty());
    assert_matches!(rep.skipped[0].1, Error::InsideAlternateContent);
    assert_eq!(p.to_bytes().unwrap(), before);

    // every Phase 0 refusal on a notes body, byte-identical
    let mut p = Presentation::from_bytes(sample_with_notes_text()).unwrap();
    type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>;
    let mut q = Presentation::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    assert_matches!(refused(&mut q, |p| p.notes_mut(0)?.body_mut()?.remove_paragraph(0)), Error::LastParagraph);
    type Want = fn(&Error) -> bool;
    let ops: [(Op, Want); 8] = [
        (|b| b.paragraph_mut(5).map(|_| ()), |e| matches!(e, Error::ParagraphNotFound(5))),
        (|b| b.paragraph_mut(0)?.run_mut(7).map(|_| ()), |e| matches!(e, Error::RunNotFound(7))),
        (|b| b.paragraph_mut(0)?.remove_field(0), |e| matches!(e, Error::FieldNotFound(0))),
        (|b| b.paragraph_mut(0)?.set_text("a\nb"), |e| matches!(e, Error::LineBreakInParagraph)),
        (|b| b.paragraph_mut(0)?.run_mut(0)?.set_text("a\u{b}b"), |e| matches!(e, Error::LineBreakInRun)),
        (|b| b.set_text("x\u{1}"), |e| matches!(e, Error::InvalidXmlChar { .. })),
        (|b| b.add_paragraph("\u{FFFE}").map(|_| ()), |e| matches!(e, Error::InvalidXmlChar { .. })),
        (|b| b.paragraph_mut(0)?.add_run("\u{0}"), |e| matches!(e, Error::InvalidXmlChar { .. })),
    ];
    for (k, (op, want)) in ops.into_iter().enumerate() {
        let e = refused(&mut p, |p| op(&mut p.notes_mut(0)?.body_mut()?));
        assert!(want(&e), "op {k}: {e:?}");
    }
    // SYNTHETIC: a field, unsupported content, a format that cannot be copied
    let field = patched("poi/sample.pptx", &[(NOTES1, "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", "<a:p><a:fld id=\"{975BB87B-4768-4521-991C-A3C89D1764D9}\" type=\"slidenum\"><a:rPr lang=\"en-US\"/><a:t>1</a:t></a:fld></a:p>")]);
    let mut p = Presentation::from_bytes(field).unwrap();
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut()?.set_text("x")), Error::FieldInParagraph { paragraph: 0 });
    let math = patched("poi/sample.pptx", &[(NOTES1, "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", "<a:p><a14:m xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\"/><a:endParaRPr lang=\"en-US\"/></a:p>")]);
    let mut p = Presentation::from_bytes(math).unwrap();
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut()?.set_text("x")), Error::UnsupportedContent(_));
    let blip = patched("poi/sample.pptx", &[(NOTES1, "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", "<a:p><a:r><a:rPr lang=\"en-US\"><a:blipFill><a:blip r:embed=\"rId1\"/></a:blipFill></a:rPr><a:t>x</a:t></a:r></a:p>")]);
    let mut p = Presentation::from_bytes(blip).unwrap();
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut()?.set_text("1\n2")), Error::FormatNotCopyable("a:rPr"));
    let ns = patched("poi/sample.pptx", &[(NOTES1, "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", "<a:p><a:r xmlns:x14=\"urn:ext\"><a:rPr lang=\"en-US\"><x14:k x14:v=\"1\"/></a:rPr><a:t>x</a:t></a:r></a:p>")]);
    let mut p = Presentation::from_bytes(ns).unwrap();
    assert_matches!(refused(&mut p, |p| p.notes_mut(0)?.body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b")), Error::Core(ooxml_core::Error::NamespaceNotInScope(_)));
}

#[test]
fn find_and_replace_in_notes() {
    let mut p = Presentation::from_bytes(sample_with_notes_text()).unwrap();
    // Slides scope never looks at notes
    assert!(p.find("second", &FindOptions::default()).unwrap().is_empty());
    let all = p.find("second", &FindOptions::default().scope(Scope::SlidesAndNotes)).unwrap();
    assert_eq!(all.len(), 1);
    let m = &all[0];
    assert!(m.location.notes && m.location.slide == 0 && m.location.paragraph == 1 && m.range == (0..6));
    assert_eq!(p.find("second", &FindOptions::default().scope(Scope::Notes(0))).unwrap(), all);
    // a match across the two runs of the first notes paragraph
    let input = p.to_bytes().unwrap();
    let rep = p.replace_all("er no", "ER-NO", &ReplaceOptions::default().find(FindOptions::default().scope(Scope::Notes(0)))).unwrap();
    assert_eq!(rep.replaced.len(), 1);
    assert_eq!(p.notes(0).unwrap().unwrap().text(), "SpeakER-NOtes\nsecond line");
    let (a, b) = (Package::from_bytes(input).unwrap(), Package::from_bytes(p.to_bytes().unwrap()).unwrap());
    for n in a.part_names() {
        if n != NOTES1 {
            assert_eq!(a.read_part(n).unwrap(), b.read_part(n).unwrap(), "{n} changed");
        }
    }
    // SlidesAndNotes on a real deck: matches in both, in slide order
    let mut p = Presentation::from_bytes(cases::read_fixture("python-pptx/sld-notes.pptx")).unwrap();
    let text = p.notes(0).unwrap().unwrap().text();
    let word = text.split_whitespace().next().expect("notes text").to_owned();
    let found = p.find(&word, &FindOptions::default().scope(Scope::SlidesAndNotes)).unwrap();
    assert!(found.iter().any(|m| m.location.notes), "{found:?}");
    let rep = p.replace_all(&word, "W", &ReplaceOptions::default().find(FindOptions::default().scope(Scope::SlidesAndNotes))).unwrap();
    assert_eq!(rep.replaced.len() + rep.skipped.len(), found.len());
    assert!(!p.notes(0).unwrap().unwrap().text().contains(&word) || !rep.skipped.is_empty());
}

#[test]
fn notes_resolve_in_the_notes_master_context() {
    // §1.3 "Notes", §1.7: sz from p:notesStyle, colour map from the notes
    // master, geometry from the notes-master placeholder (by type)
    let mut p = Presentation::from_bytes(sample_with_notes_text()).unwrap();
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let n = s.notes().unwrap();
    let sheet = s.notes_sheet().unwrap();
    assert_eq!(sheet.master().unwrap().part_name(), "ppt/notesMasters/notesMaster1.xml");
    assert_eq!(sheet.theme().unwrap().part_name(), "ppt/theme/theme2.xml");
    let body = n.body_shape().unwrap();
    let tb = body.text_body().unwrap();
    let para = tb.paragraphs()[0];
    let TextItem::Run(r) = para.items()[0] else { panic!() };
    let rp = sheet.run_props(&body, tb, para, RunItem::Run(r)).unwrap();
    assert_eq!(rp.size.value(), Some(&1200));
    assert_matches!(rp.size.source(), Some(Source::MasterTextStyle { style: TextStyleKind::Notes, part: "ppt/notesMasters/notesMaster1.xml", level: 0 }));
    assert_eq!(rp.bold.value(), Some(&true));
    let Some(ooxml_pptx::Fill::Solid(c)) = rp.fill.value() else { panic!("{:?}", rp.fill) };
    assert_eq!((c.theme_slot.map(|t| t.as_str()), c.map_source), (Some("dk1"), Some(MapSource::NotesMaster)));
    let m = sheet.placeholder_match(&body).unwrap().unwrap();
    assert_eq!((m.kind, m.layout.is_none(), m.master.is_some()), (MatchKind::Type, true, true));
    let g = sheet.geometry(&body).unwrap();
    assert_matches!(g.source(), Some(Source::Placeholder { role: Role::NotesMaster, matched: MatchKind::Type, .. }));
    // the slide number placeholder: no notesStyle (body family only), no defaultTextStyle
    let num = n.shapes().into_iter().find(|x| x.name() == Some("Slide Number Placeholder 3")).unwrap();
    let tb = num.text_body().unwrap();
    let para = tb.paragraphs()[0];
    let TextItem::Field(f) = para.items()[0] else { panic!() };
    let rp = sheet.run_props(&num, tb, para, RunItem::Field(f)).unwrap();
    // notesStyle's lvl1 sets kern="1200": not reached by a non-body placeholder
    assert_eq!(rp.kern, Lookup::Unset, "{:?}", rp.kern);
    match rp.size {
        Lookup::Set(r) => assert_matches!(r.source, Source::Placeholder { role: Role::NotesMaster, .. } | Source::Direct),
        Lookup::Unset => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn notes_color_map_override() {
    // SYNTHETIC: the notes slide's own override wins
    let bytes = patched(
        "poi/sample.pptx",
        &[(NOTES1, "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>", "<p:clrMapOvr><a:overrideClrMapping bg1=\"dk1\" tx1=\"lt1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/></p:clrMapOvr>")],
    );
    let mut p = Presentation::from_bytes(bytes).unwrap();
    let deck = p.deck().unwrap();
    let sheet = deck.slide(0).unwrap().notes_sheet().unwrap();
    let m = sheet.color_map().unwrap().unwrap();
    assert_eq!((m.from, m.node.attr(ooxml_core::QName::new("", "tx1"))), (MapSource::NotesOverride, Some("lt1")));
}

#[test]
fn layouts_and_masters_as_contexts() {
    let mut p = Presentation::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let deck = p.deck().unwrap();
    let masters = deck.masters();
    let layouts = deck.layouts();
    assert_eq!(masters.len(), 1);
    assert_eq!(layouts.len(), 7);
    assert!(layouts.iter().all(|l| l.kind() == SheetKind::Layout && l.master().unwrap().part_name() == "ppt/slideMasters/slideMaster1.xml"));
    // the title layout's title: master placeholder (family), then titleStyle
    let title_layout = layouts.iter().find(|l| l.part().part_name() == "ppt/slideLayouts/slideLayout1.xml").unwrap();
    let shapes = title_layout.shapes();
    let t = shapes.iter().find(|s| s.placeholder().is_some_and(|ph| ph.view().raw().attr(ooxml_core::QName::new("", "type")) == Some("ctrTitle"))).unwrap();
    let m = title_layout.placeholder_match(t).unwrap().unwrap();
    assert_eq!((m.kind, m.layout.is_none(), m.master.is_some()), (MatchKind::MasterFamily, true, true));
    let tb = t.text_body().unwrap();
    let para = tb.paragraphs()[0];
    let rp = title_layout.run_props(t, tb, para, RunItem::End).unwrap();
    assert!(rp.size.value().is_some());
    assert_matches!(rp.size.source(), Some(Source::Direct | Source::ShapeListStyle { .. } | Source::Placeholder { role: Role::Master, .. } | Source::MasterTextStyle { style: TextStyleKind::Title, .. }));
    let g = title_layout.geometry(t).unwrap();
    assert!(g.value().is_some());
    // the layout's colour map: the master's (no override in sample.pptx)
    assert_eq!(title_layout.color_map().unwrap().unwrap().from, MapSource::Master);
    // a master placeholder: OnMaster, txStyles
    let master = masters[0];
    assert_eq!(master.kind(), SheetKind::Master);
    let shapes = master.shapes();
    let body = shapes.iter().find(|s| s.placeholder().is_some_and(|ph| ph.view().raw().attr(ooxml_core::QName::new("", "type")) == Some("body"))).unwrap();
    assert_eq!(master.placeholder_match(body).unwrap().unwrap().kind, MatchKind::OnMaster);
    let tb = body.text_body().unwrap();
    let para = tb.paragraphs()[0];
    let rp = master.run_props(body, tb, para, RunItem::End).unwrap();
    assert_matches!(rp.size.source(), Some(Source::Direct | Source::ParagraphDefault | Source::ShapeListStyle { .. } | Source::MasterTextStyle { style: TextStyleKind::Body, .. }));
    assert_matches!(master.geometry(body).unwrap().source(), Some(Source::Direct));
    assert_eq!(master.color_map().unwrap().unwrap().from, MapSource::Master);
}

#[test]
fn context_rules_synthetic() {
    // SYNTHETIC (§1.7): a layout title without its own size and a master
    // title placeholder lstStyle with sz=5000 → the layout title resolves
    // from the master placeholder; a text box on the layout resolves
    // defaultTextStyle, then otherStyle.
    const LAYOUT: &str = "ppt/slideLayouts/slideLayout1.xml";
    const MASTER: &str = "ppt/slideMasters/slideMaster1.xml";
    let master = String::from_utf8(Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap().read_part(MASTER).unwrap()).unwrap();
    // the master title placeholder's first a:lstStyle
    let title_at = master.find("type=\"title\"").unwrap();
    let lst = master[title_at..].find("<a:lstStyle/>").map(|k| k + title_at).expect("empty lstStyle on the master title");
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let m2 = format!("{}<a:lstStyle><a:lvl1pPr><a:defRPr sz=\"5000\"/></a:lvl1pPr></a:lstStyle>{}", &master[..lst], &master[lst + "<a:lstStyle/>".len()..]);
    pkg.set_part(MASTER, m2.into_bytes()).unwrap();
    let layout = String::from_utf8(pkg.read_part(LAYOUT).unwrap()).unwrap();
    let tb = "<p:sp><p:nvSpPr><p:cNvPr id=\"90\" name=\"Layout text box\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>box</a:t></a:r></a:p></p:txBody></p:sp></p:spTree>";
    pkg.set_part(LAYOUT, layout.replacen("</p:spTree>", tb, 1).into_bytes()).unwrap();
    let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let deck = p.deck().unwrap();
    let l = deck.layouts().into_iter().find(|l| l.part().part_name() == LAYOUT).unwrap();
    let shapes = l.shapes();
    let title = shapes.iter().find(|s| s.placeholder().is_some_and(|ph| ph.view().raw().attr(ooxml_core::QName::new("", "type")) == Some("ctrTitle"))).unwrap();
    let tbody = title.text_body().unwrap();
    let para = tbody.paragraphs()[0];
    let own_size = child_sz(para.raw());
    let rp = l.run_props(title, tbody, para, RunItem::End).unwrap();
    assert!(!own_size);
    assert_eq!(rp.size.value(), Some(&5000), "{:?}", rp.size);
    assert_matches!(rp.size.source(), Some(Source::Placeholder { role: Role::Master, matched: MatchKind::MasterFamily, .. }));
    let bx = shapes.iter().find(|s| s.name() == Some("Layout text box")).unwrap();
    let bbody = bx.text_body().unwrap();
    let para = bbody.paragraphs()[0];
    let TextItem::Run(r) = para.items()[0] else { panic!() };
    let rp = l.run_props(bx, bbody, para, RunItem::Run(r)).unwrap();
    assert_matches!(rp.latin.source(), Some(Source::PresentationDefault { level: 0 }));
    assert!(l.placeholder_match(bx).unwrap().is_none());
}

/// Does the paragraph (or its end) carry its own `sz`?
fn child_sz(p: &ooxml_core::RawNode) -> bool {
    p.elements().any(|c| c.elements().any(|x| x.attr(ooxml_core::QName::new("", "sz")).is_some()) || c.attr(ooxml_core::QName::new("", "sz")).is_some())
}

/// Opt-in corpus sweep (`OOXML_PPTX_CORPUS=dir:dir … -- --ignored`): on
/// every notes body, a body `set_text`, a paragraph `set_text` with a line
/// break and a deck-wide notes replace either succeed — only the notes part
/// changes, the text reads back — or are refused byte-identically.
#[test]
#[ignore = "needs OOXML_PPTX_CORPUS"]
fn corpus_notes_edits() {
    let Ok(dirs) = std::env::var("OOXML_PPTX_CORPUS") else { return };
    let (mut files, mut bodies, mut ok, mut refused_n) = (0, 0, 0, std::collections::BTreeMap::<String, usize>::new());
    for d in dirs.split(':') {
        let mut entries: Vec<_> = std::fs::read_dir(d).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries.into_iter().filter(|p| p.extension().is_some_and(|e| e == "pptx")) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut p) = Presentation::from_bytes(bytes.clone()) else { continue };
            files += 1;
            for i in 0..p.slide_count() {
                let Ok(Some(n)) = p.notes(i) else { continue };
                if n.body().is_none() {
                    continue;
                }
                let part = n.part_name().to_owned();
                bodies += 1;
                type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>;
                let ops: [(&str, Op, &str); 2] = [("body", |b| b.set_text("Note A\nNote B\u{b}C"), "Note A\nNote B\u{b}C"), ("para", |b| b.paragraph_mut(0)?.set_text("P\u{b}Q"), "P\u{b}Q")];
                for (what, op, want) in ops {
                    let mut q = Presentation::from_bytes(bytes.clone()).unwrap();
                    let before = q.to_bytes().unwrap();
                    let r = q.notes_mut(i).and_then(|mut nm| op(&mut nm.body_mut()?));
                    match r {
                        Ok(()) => {
                            let out = q.to_bytes().unwrap();
                            let mut back = Presentation::from_bytes(out.clone()).unwrap();
                            let text = back.notes(i).unwrap().unwrap().text();
                            assert!(if what == "body" { text == want } else { text.starts_with(want) }, "{}: {what}: {text:?}", path.display());
                            let (a, b) = (Package::from_bytes(before).unwrap(), Package::from_bytes(out).unwrap());
                            for name in a.part_names() {
                                if name != part {
                                    assert!(a.read_part(name).unwrap() == b.read_part(name).unwrap(), "{}: {name} changed", path.display());
                                }
                            }
                            ok += 1;
                        }
                        Err(e) => {
                            assert!(q.to_bytes().unwrap() == before, "{}: refused {what} changed the package", path.display());
                            *refused_n.entry(format!("{e:?}").split([' ', '(', '{']).next().unwrap().to_owned()).or_default() += 1;
                        }
                    }
                }
            }
            // a deck-wide replace over notes only
            let mut q = Presentation::from_bytes(bytes.clone()).unwrap();
            for i in 0..q.slide_count() {
                let o = ReplaceOptions::default().find(FindOptions::default().scope(Scope::Notes(i)));
                let before = q.to_bytes().unwrap();
                match q.replace_all("e", "E", &o) {
                    Ok(_) => {}
                    Err(_) => assert!(q.to_bytes().unwrap() == before),
                }
            }
        }
    }
    eprintln!("{files} files, {bodies} notes bodies, {ok} edits applied, refused {refused_n:?}");
}

/// §3.1 "on slides and notes" and §1.7: links in notes, layouts and masters
/// (review #56 M1, deferred to this PR).
#[test]
fn links_in_notes_layouts_and_masters() {
    let link = r#"<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=lastslide"/>"#;
    let bytes = patched(
        "poi/sample.pptx",
        &[
            (NOTES1, "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", &format!("<a:p><a:r><a:rPr lang=\"en-US\">{link}</a:rPr><a:t>jump</a:t></a:r></a:p>")),
            ("ppt/slideLayouts/slideLayout1.xml", "<p:cNvPr id=\"25\" name=\"Rectangle 3\"/>", &format!("<p:cNvPr id=\"25\" name=\"Rectangle 3\">{link}</p:cNvPr>")),
        ],
    );
    let mut p = Presentation::from_bytes(bytes).unwrap();
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let nl = s.notes_link_reader().unwrap().links();
    assert_eq!(nl.len(), 1);
    assert_eq!(nl[0].1.target, ooxml_pptx::LinkTarget::Jump(ooxml_pptx::ShowJump::Last));
    assert_eq!(nl[0].0.text, Some((0, 0)));
    assert!(s.links().is_empty() || s.links().iter().all(|(_, l)| l.target != nl[0].1.target));
    let layout = deck.layouts().into_iter().find(|l| l.part().part_name() == "ppt/slideLayouts/slideLayout1.xml").unwrap();
    let ll = deck.link_reader(&layout).links();
    assert_eq!(ll.len(), 1);
    assert_eq!(ll[0].0.text, None);
    assert!(deck.masters().iter().all(|m| deck.link_reader(m).links().is_empty()));
}

/// N5: a non-placeholder text box on a notes slide resolves steps 1–4 only
/// (no p:defaultTextStyle, whose lvl1 sets latin Arial in sample.pptx).
#[test]
fn notes_text_box_has_no_presentation_default() {
    let tb = r#"<p:sp><p:nvSpPr><p:cNvPr id="40" name="Notes box"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>box</a:t></a:r></a:p></p:txBody></p:sp></p:spTree>"#;
    let mut p = Presentation::from_bytes(patched("poi/sample.pptx", &[(NOTES1, "</p:spTree>", tb)])).unwrap();
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let sheet = s.notes_sheet().unwrap();
    let shapes = sheet.shapes();
    let bx = shapes.iter().find(|x| x.name() == Some("Notes box")).unwrap();
    let b = bx.text_body().unwrap();
    let para = b.paragraphs()[0];
    let TextItem::Run(r) = para.items()[0] else { panic!() };
    let rp = sheet.run_props(bx, b, para, RunItem::Run(r)).unwrap();
    assert_eq!(rp.latin, Lookup::Unset, "{:?}", rp.latin);
    assert_eq!(rp.size, Lookup::Unset);
}
