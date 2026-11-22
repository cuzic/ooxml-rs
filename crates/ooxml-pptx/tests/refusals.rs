//! T4 (ADR-0005 §7): every refusal of §6 on a fixture, asserting that the
//! package is byte-identical after the refused call. Where no real file
//! has the construct, the test derives a synthetic input from a real
//! fixture by one documented, minimal XML edit made in memory (nothing is
//! written to `reference/`). Each synthetic edit is named `SYNTHETIC:`.

#[path = "oracle/cases.rs"]
mod cases;

use std::assert_matches;

use ooxml_core::{Package, RawNode};
use ooxml_pptx::{Error, Presentation, ShapeKind, ShapePath};

fn open(f: &str) -> Presentation {
    Presentation::from_bytes(cases::read_fixture(f)).unwrap()
}

/// Replaces one part of a fixture in memory.
fn patch(f: &str, part: &str, edit: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut pkg = Package::from_bytes(cases::read_fixture(f)).unwrap();
    let xml = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    let new = edit(xml.clone());
    assert_ne!(new, xml, "the synthetic edit must change {part}");
    pkg.set_part(part, new.into_bytes()).unwrap();
    pkg.to_bytes().unwrap()
}

fn replace_once(s: &str, from: &str, to: &str) -> String {
    assert!(s.contains(from), "pattern {from:?} not found");
    s.replacen(from, to, 1)
}

/// Runs a refused call and checks nothing changed.
fn refused<T: std::fmt::Debug>(pres: &mut Presentation, call: impl FnOnce(&mut Presentation) -> Result<T, Error>) -> Error {
    let before = pres.to_bytes().unwrap();
    let err = call(pres).expect_err("the call must be refused");
    assert_eq!(pres.to_bytes().unwrap(), before, "refused call changed the package ({err})");
    err
}

fn path_of(pres: &mut Presentation, slide: usize, pred: impl Fn(&ooxml_pptx::ShapeRef<'_>) -> bool) -> ShapePath {
    let s = pres.slide(slide).unwrap();
    s.shapes().into_iter().find(|x| pred(x)).expect("shape").path().clone()
}

fn first_shape_id(pres: &mut Presentation, slide: usize) -> u32 {
    let s = pres.slide(slide).unwrap();
    s.shapes().into_iter().find(|x| x.text_body().is_some()).unwrap().id().unwrap()
}

#[test]
fn package_level() {
    // a real non-PowerPoint package
    let xlsx = std::fs::read(cases::fixtures_dir().join("../../round_trip_corpus/fixture_minimal.xlsx")).unwrap();
    assert_matches!(Presentation::from_bytes(xlsx), Err(Error::NotAPresentation(_)));
    // SYNTHETIC: the 8-byte CFB signature (encrypted/legacy Office file)
    let mut cfb = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    cfb.resize(4096, 0);
    assert_matches!(Presentation::from_bytes(cfb), Err(Error::Core(ooxml_core::Error::EncryptedPackage)));
    // SYNTHETIC: sample.pptx with conformance="strict" on p:presentation
    let strict = patch("poi/sample.pptx", "ppt/presentation.xml", |x| replace_once(&x, "<p:presentation ", "<p:presentation conformance=\"strict\" "));
    assert_matches!(Presentation::from_bytes(strict), Err(Error::StrictConformance));
    // SYNTHETIC: the first sldId's r:id points to no relationship
    let dangling = patch("poi/sample.pptx", "ppt/presentation.xml", |x| {
        let at = x.find("<p:sldId ").unwrap();
        let rid = x[at..].find("r:id=\"").unwrap() + at + 6;
        format!("{}rIdMissing{}", &x[..rid], &x[rid + x[rid..].find('"').unwrap()..])
    });
    let mut p = Presentation::from_bytes(dangling).unwrap();
    assert_matches!(p.slides().next(), Some(Err(Error::DanglingSlide { .. })));
    assert_matches!(p.slide(0), Err(Error::DanglingSlide { .. }));
    assert_matches!(p.slide_mut(0), Err(Error::DanglingSlide { .. }));
    assert!(p.slide(1).is_ok(), "the other slide is still readable");
    let mut p = open("poi/sample.pptx");
    assert_matches!(p.slide(99), Err(Error::SlideIndexOutOfRange { index: 99, count: 2 }));
}

#[test]
fn shape_addressing() {
    let mut p = open("poi/sample.pptx");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(999_999).map(|_| ()));
    assert_matches!(e, Error::ShapeNotFound);
    // Real: bug62513.pptx slide 17 has cNvPr id 0 three times, all inside
    // an OLE object's mc:Fallback within a:graphicData: not shape ids
    // (review M3), so they are neither found nor ambiguous.
    let mut p = open("poi/bug62513.pptx");
    assert!(p.slide(16).unwrap().shape_by_id(0).unwrap().is_none());
    // SYNTHETIC: sample.pptx slide 1 with the second shape's id set to the first's
    let dup = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| replace_once(&x, "id=\"2050\"", "id=\"2049\""));
    let mut p = Presentation::from_bytes(dup).unwrap();
    assert_matches!(p.slide(0).unwrap().shape_by_id(2049), Err(Error::AmbiguousShapeId { id: 2049 }));
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049).map(|_| ()));
    assert_matches!(e, Error::AmbiguousShapeId { id: 2049 });
    // SYNTHETIC: the title's cNvPr without @id
    let noid = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| replace_once(&x, "id=\"2049\" ", ""));
    let mut p = Presentation::from_bytes(noid).unwrap();
    let path = path_of(&mut p, 0, |s| s.placeholder().is_some());
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut(&path).map(|_| ()));
    assert_matches!(e, Error::InvalidShapeId);
    // a path that is not a shape
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut(&ShapePath::from_slice(&[0])).map(|_| ()));
    assert_matches!(e, Error::ShapeNotFound);
}

#[test]
fn alternate_content_is_read_only() {
    // Real: Of16-01.pptx, a cx:chart in mc:AlternateContent directly under
    // p:spTree; its picture fallback carries the same cNvPr id 7.
    let mut p = open("open-xml-sdk/Of16-01.pptx");
    let (si, path, id) = (0..p.slide_count())
        .find_map(|i| {
            let s = p.slide(i).unwrap();
            s.shapes().into_iter().find(|x| *x.kind() == ShapeKind::AlternateContent).map(|x| (i, x.path().clone(), x.id().unwrap()))
        })
        .expect("an AC entry");
    assert_eq!(id, 7);
    assert!(p.slide(si).unwrap().shape_by_id(7).unwrap().is_some(), "AC branches count once");
    assert_matches!(refused(&mut p, |p| p.slide_mut(si)?.shape_mut(&path).map(|_| ())), Error::InsideAlternateContent);
    assert_matches!(refused(&mut p, |p| p.slide_mut(si)?.shape_mut_by_id(7).map(|_| ())), Error::InsideAlternateContent);
    let mut deeper = path.as_slice().to_vec();
    deeper.extend([0, 0]);
    assert_matches!(
        refused(&mut p, |p| p.slide_mut(si)?.shape_mut(&ShapePath::from_slice(&deeper)).map(|_| ())),
        Error::InsideAlternateContent
    );
}

#[test]
fn positioned_shape_insertion_refuses_same_slot_alternate_content() {
    use ooxml_core::{Error as CoreError, XmlDocument};
    use ooxml_schema::pml::{CommonSlideData, Shape, ShapeTree, Slide};

    let input = cases::read_fixture("open-xml-sdk/Of16-01.pptx");
    let pkg = Package::from_bytes(input).unwrap();
    for part in pkg.part_names().filter(|p| p.starts_with("ppt/slides/slide") && p.ends_with(".xml")) {
        let source = pkg.read_part(part).unwrap();
        let mut doc = XmlDocument::parse(&source).unwrap();
        let mut slide = doc.lens_mut::<Slide>().unwrap();
        let mut common = slide.child_mut::<CommonSlideData>().unwrap();
        let mut tree = common.child_mut::<ShapeTree>().unwrap();
        if tree.node().elements().any(|n| n.is(ooxml_core::name::MC_ALTERNATE_CONTENT)) {
            assert_matches!(tree.insert_by::<Shape>(|index, _| index == 0), Err(CoreError::UnresolvedSibling(_)));
            assert_eq!(doc.serialize().as_bytes(), source);
            return;
        }
    }
    panic!("fixture must have a slide with AlternateContent in its shape tree");
}

#[test]
fn bodies_tables_cells() {
    // Real: shapes.pptx has connectors, groups and tables (with merged cells)
    let mut p = open("poi/shapes.pptx");
    let mut found = (false, false, false);
    for i in 0..p.slide_count() {
        let shapes: Vec<(ShapeKind, ShapePath)> = p.slide(i).unwrap().shapes().iter().map(|s| (s.kind().clone(), s.path().clone())).collect();
        for (kind, path) in shapes {
            match kind {
                ShapeKind::Connector | ShapeKind::Group | ShapeKind::Picture => {
                    assert_matches!(refused(&mut p, |p| p.slide_mut(i)?.shape_mut(&path)?.text_body_mut().map(|_| ())), Error::NoTextBody);
                    assert_matches!(refused(&mut p, |p| p.slide_mut(i)?.shape_mut(&path)?.table_cell_mut(0, 0).map(|_| ())), Error::NotATable);
                    found.0 = true;
                }
                ShapeKind::Table => {
                    assert_matches!(
                        refused(&mut p, |p| p.slide_mut(i)?.shape_mut(&path)?.table_cell_mut(99, 0).map(|_| ())),
                        Error::CellNotFound { row: 99, col: 0 }
                    );
                    assert_matches!(refused(&mut p, |p| p.slide_mut(i)?.shape_mut(&path)?.text_body_mut().map(|_| ())), Error::NoTextBody);
                    found.1 = true;
                }
                ShapeKind::Shape => found.2 = true,
                _ => {}
            }
        }
    }
    assert_eq!(found, (true, true, true));
    // Real: table_test2.pptx, 16 cells covered by merges
    let mut p = open("poi/table_test2.pptx");
    let path = path_of(&mut p, 0, |s| *s.kind() == ShapeKind::Table);
    let merged: Vec<(usize, usize)> = {
        let s = p.slide(0).unwrap();
        let t = s.shapes().into_iter().find(|x| *x.kind() == ShapeKind::Table).unwrap().table().unwrap();
        t.rows()
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.iter().enumerate().filter(|(_, c)| c.is_merged().unwrap()).map(move |(c, _)| (r, c)).collect::<Vec<_>>())
            .collect()
    };
    assert_eq!(merged.len(), 16);
    for (r, c) in merged {
        let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut(&path)?.table_cell_mut(r, c).map(|_| ()));
        assert_matches!(e, Error::MergedCell { .. });
    }
}

#[test]
fn paragraph_level() {
    let mut p = open("poi/sample.pptx");
    let id = first_shape_id(&mut p, 0); // the title: one paragraph, one run
    let e = |p: &mut Presentation, f: &dyn Fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>| {
        refused(p, |p| {
            let mut s = p.slide_mut(0)?;
            let mut sh = s.shape_mut_by_id(id)?;
            f(&mut sh.text_body_mut()?)
        })
    };
    assert_matches!(e(&mut p, &|b| b.remove_paragraph(0)), Error::LastParagraph);
    assert_matches!(e(&mut p, &|b| b.paragraph_mut(5).map(|_| ())), Error::ParagraphNotFound(5));
    assert_matches!(e(&mut p, &|b| b.paragraph_mut(0)?.run_mut(3).map(|_| ())), Error::RunNotFound(3));
    assert_matches!(e(&mut p, &|b| b.paragraph_mut(0)?.remove_run(3)), Error::RunNotFound(3));
    assert_matches!(e(&mut p, &|b| b.paragraph_mut(0)?.remove_field(0)), Error::FieldNotFound(0));

    // Real: layouts.pptx slide 10 has a paragraph that is only an a:fld
    let mut p = open("poi/layouts.pptx");
    let (si, fid) = (0..p.slide_count())
        .find_map(|i| {
            let s = p.slide(i).unwrap();
            s.shapes().into_iter().find(|x| {
                x.text_body().is_some_and(|b| b.paragraphs().iter().any(|q| q.items().iter().any(|t| matches!(t, ooxml_pptx::TextItem::Field(_)))))
            }).map(|x| (i, x.id().unwrap()))
        })
        .expect("a field");
    let body_op = |p: &mut Presentation, f: &dyn Fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>| {
        refused(p, |p| {
            let mut s = p.slide_mut(si)?;
            let mut sh = s.shape_mut_by_id(fid)?;
            f(&mut sh.text_body_mut()?)
        })
    };
    assert_matches!(body_op(&mut p, &|b| b.paragraph_mut(0)?.set_text("x")), Error::FieldInParagraph { paragraph: 0 });
    assert_matches!(body_op(&mut p, &|b| b.set_text("x")), Error::FieldInParagraph { paragraph: 0 });
    assert_matches!(body_op(&mut p, &|b| b.set_text("x\ny")), Error::FieldInParagraph { paragraph: 0 });
    // explicit removal of the field, then the paragraph can be rewritten
    {
        let mut s = p.slide_mut(si).unwrap();
        let mut sh = s.shape_mut_by_id(fid).unwrap();
        let mut b = sh.text_body_mut().unwrap();
        b.paragraph_mut(0).unwrap().remove_field(0).unwrap();
        b.set_text("no field").unwrap();
        assert_eq!(b.as_ref().text(), "no field");
    }

    // SYNTHETIC: an a14:m (Office math) element inside the title paragraph of sample.pptx
    let math = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        replace_once(&x, "<a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp>", "<a14:m xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\"/><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp>")
    });
    let mut p = Presentation::from_bytes(math).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("x"));
    assert_matches!(e, Error::UnsupportedContent(_));
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::UnsupportedContent(_));
}

/// A comparable form of an element subtree: local names and attributes
/// (sorted), namespace declarations left out, children in order; the root's
/// name is left out so `a:endParaRPr` and its copy as `a:rPr` compare equal.
fn subtree(n: &RawNode, skip: &[&str]) -> String {
    fn inner(n: &RawNode, out: &mut String) {
        let mut attrs: Vec<String> = n
            .attrs()
            .iter()
            .filter(|a| !a.name().is_namespace_declaration())
            .map(|a| format!("{{{}}}{}={:?}", a.name().ns(), a.name().local(), a.value()))
            .collect();
        attrs.sort();
        out.push_str(&format!("[{}]", attrs.join(",")));
        for c in n.elements() {
            out.push_str(&format!("<{{{}}}{}", c.name().ns(), c.name().local()));
            inner(c, out);
            out.push('>');
        }
    }
    let mut out = String::new();
    let mut attrs: Vec<String> = n
        .attrs()
        .iter()
        .filter(|a| !a.name().is_namespace_declaration())
        .map(|a| format!("{{{}}}{}={:?}", a.name().ns(), a.name().local(), a.value()))
        .collect();
    attrs.sort();
    out.push_str(&format!("[{}]", attrs.join(",")));
    for k in n.elements().filter(|k| !skip.contains(&k.name().local())) {
        out.push_str(&format!("<{{{}}}{}", k.name().ns(), k.name().local()));
        inner(k, &mut out);
        out.push('>');
    }
    out
}

fn child<'a>(n: &'a RawNode, local: &str) -> Option<&'a RawNode> {
    n.elements().find(|c| c.name().local() == local)
}

fn body_of<'a>(s: &ooxml_pptx::Slide<'a>, id: u32) -> Vec<ooxml_pptx::ParagraphRef<'a>> {
    s.shape_by_id(id).unwrap().unwrap().text_body().unwrap().paragraphs()
}

#[test]
fn format_sources() {
    // Phase 0.1 (ADR-0005 §3 "Format sources"): sources with child elements
    // are copied as subtrees.
    //
    // Real: sample_pptx_grouping_issues.pptx has empty paragraphs whose
    // a:endParaRPr has child elements: the new run's a:rPr is a copy of it.
    let mut p = open("poi/sample_pptx_grouping_issues.pptx");
    let target = {
        let s = p.slide(0).unwrap();
        s.shapes()
            .into_iter()
            .find(|x| {
                x.text_body().is_some_and(|b| {
                    let q = b.paragraphs()[0];
                    q.runs().is_empty() && child(q.raw(), "endParaRPr").is_some_and(|c| c.elements().next().is_some())
                })
            })
            .map(|x| x.id().unwrap())
            .expect("an empty paragraph with a formatted endParaRPr")
    };
    let want = subtree(child(body_of(&p.slide(0).unwrap(), target)[0].raw(), "endParaRPr").unwrap(), &[]);
    p.slide_mut(0).unwrap().shape_mut_by_id(target).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().set_text("x").unwrap();
    {
        let s = p.slide(0).unwrap();
        let q = body_of(&s, target)[0];
        let run = child(q.raw(), "r").unwrap();
        assert_eq!(subtree(child(run, "rPr").unwrap(), &[]), want);
        assert_eq!(subtree(child(q.raw(), "endParaRPr").unwrap(), &[]), want, "endParaRPr kept");
    }
    // Real: sample.pptx's title run has <a:sym> in its a:rPr and an
    // attribute-only a:pPr; "\v" and new lines copy both.
    let mut p = open("poi/sample.pptx");
    let (rpr, ppr, end) = {
        let s = p.slide(0).unwrap();
        let q = body_of(&s, 2049)[0].raw();
        (
            subtree(child(child(q, "r").unwrap(), "rPr").unwrap(), &[]),
            subtree(child(q, "pPr").unwrap(), &[]),
            subtree(child(q, "endParaRPr").unwrap(), &[]),
        )
    };
    assert_ne!(rpr, end);
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("a\u{b}b\nc\n").unwrap();
    {
        let s = p.slide(0).unwrap();
        let paras = body_of(&s, 2049);
        assert_eq!(paras.len(), 3);
        let runs: Vec<&RawNode> = paras.iter().flat_map(|q| q.raw().elements().filter(|c| c.name().local() == "r")).collect();
        assert_eq!(runs.len(), 3, "a, b, c");
        for r in runs {
            assert_eq!(subtree(child(r, "rPr").unwrap(), &[]), rpr);
        }
        for q in &paras[1..] {
            assert_eq!(subtree(child(q.raw(), "pPr").unwrap(), &[]), ppr);
            assert_eq!(subtree(child(q.raw(), "endParaRPr").unwrap(), &[]), end, "N3-1: the last paragraph's own a:endParaRPr");
        }
    }
    // SYNTHETIC: the same title with an empty a:endParaRPr (no format of its
    // own): new paragraphs take the run format source instead (R2-5).
    let bare = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        replace_once(&x, "</a:r><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp>", "</a:r><a:endParaRPr/></a:p></p:txBody></p:sp><p:sp>")
    });
    let mut q = Presentation::from_bytes(bare).unwrap();
    q.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("a\nb").unwrap();
    assert_eq!(subtree(child(body_of(&q.slide(0).unwrap(), 2049)[1].raw(), "endParaRPr").unwrap(), &[]), rpr);
    // The subtitle's first a:pPr has an a:spcAft child: new paragraphs copy it.
    p.slide_mut(0).unwrap().shape_mut_by_id(2050).unwrap().text_body_mut().unwrap().set_text("only").unwrap();
    let ppr = subtree(child(body_of(&p.slide(0).unwrap(), 2050)[0].raw(), "pPr").unwrap(), &[]);
    assert!(ppr.contains("spcAft"));
    p.slide_mut(0).unwrap().shape_mut_by_id(2050).unwrap().text_body_mut().unwrap().set_text("1\n2").unwrap();
    assert_eq!(subtree(child(body_of(&p.slide(0).unwrap(), 2050)[1].raw(), "pPr").unwrap(), &[]), ppr);
}

#[test]
fn hyperlinks_stay_on_their_run() {
    // Real: shapes.pptx slide 5 has a table whose cells' first runs carry
    // a:hlinkClick (r:id). New runs and new paragraphs copy the format
    // without the link (skip list), so the r:id is not duplicated.
    let mut p = open("poi/shapes.pptx");
    let (id, cells) = {
        let s = p.slide(4).unwrap();
        let t = s.shapes().into_iter().find(|x| *x.kind() == ShapeKind::Table).unwrap();
        let mut cells = Vec::new();
        for (r, row) in t.table().unwrap().rows().iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let linked = cell.text_body().is_some_and(|b| {
                    b.paragraphs().first().and_then(|q| child(q.raw(), "r")).and_then(|r| child(r, "rPr")).is_some_and(|rp| child(rp, "hlinkClick").is_some())
                });
                if linked {
                    cells.push((r, c));
                }
            }
        }
        (t.id().unwrap(), cells)
    };
    assert_eq!(cells.len(), 3);
    let cell_body = |p: &mut Presentation, (r, c): (usize, usize)| {
        let s = p.slide(4).unwrap();
        let t = s.shape_by_id(id).unwrap().unwrap().table().unwrap();
        let b = t.cell(r, c).unwrap().text_body().unwrap();
        b.paragraphs().iter().map(|q| q.raw().clone()).collect::<Vec<RawNode>>()
    };
    let before = cell_body(&mut p, cells[0]);
    let rpr = child(child(&before[0], "r").unwrap(), "rPr").unwrap().clone();
    {
        let mut s = p.slide_mut(4).unwrap();
        let mut sh = s.shape_mut_by_id(id).unwrap();
        sh.table_cell_mut(cells[0].0, cells[0].1).unwrap().paragraph_mut(0).unwrap().set_text("link\u{b}plain").unwrap();
        sh.table_cell_mut(cells[1].0, cells[1].1).unwrap().set_text("link\nplain").unwrap();
    }
    let after = cell_body(&mut p, cells[0]);
    let runs: Vec<&RawNode> = after[0].elements().filter(|c| c.name().local() == "r").collect();
    assert_eq!(subtree(child(runs[0], "rPr").unwrap(), &[]), subtree(&rpr, &[]), "the first run keeps its link");
    assert_eq!(subtree(child(runs[1], "rPr").unwrap(), &[]), subtree(&rpr, &["hlinkClick"]));
    assert!(child(child(runs[1], "rPr").unwrap(), "hlinkClick").is_none());
    let after = cell_body(&mut p, cells[1]);
    assert!(child(child(child(&after[0], "r").unwrap(), "rPr").unwrap(), "hlinkClick").is_some());
    let new_run_rpr = child(child(&after[1], "r").unwrap(), "rPr").unwrap();
    assert!(child(new_run_rpr, "hlinkClick").is_none() && new_run_rpr.attr(ooxml_core::QName::new("", "lang")) == Some("en-US"));
    assert!(child(child(&after[1], "endParaRPr").unwrap(), "hlinkClick").is_none());
}

#[test]
fn format_not_copyable() {
    // What detach_as refuses stays FormatNotCopyable; single-line edits that
    // reuse the run are unaffected.
    let title_rpr = "<a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr><a:t>Lorem";
    let cases: [(&str, &str, &str); 4] = [
        // SYNTHETIC: a picture fill (r:embed) in the title's a:rPr
        ("blip fill with r:embed", title_rpr, "<a:rPr lang=\"en-US\"><a:blipFill><a:blip r:embed=\"rId1\"/></a:blipFill><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr><a:t>Lorem"),
        // SYNTHETIC: mc:AlternateContent in the title's a:rPr
        ("AlternateContent", title_rpr, "<a:rPr lang=\"en-US\"><mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Fallback/></mc:AlternateContent></a:rPr><a:t>Lorem"),
        // SYNTHETIC: a namespace declared on an element inside the a:rPr
        ("namespace declaration", title_rpr, "<a:rPr lang=\"en-US\"><a:sym xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" typeface=\"Wingdings\"/></a:rPr><a:t>Lorem"),
        // SYNTHETIC: a picture bullet (r:embed) in the title's a:pPr
        ("bullet picture", "<a:pPr hangingPunct=\"1\"/>", "<a:pPr hangingPunct=\"1\"><a:buBlip><a:blip r:embed=\"rId1\"/></a:buBlip></a:pPr>"),
    ];
    for (what, from, to) in cases {
        let bytes = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| replace_once(&x, from, to));
        let mut p = Presentation::from_bytes(bytes).unwrap();
        let want = if what == "bullet picture" { "a:pPr" } else { "a:rPr" };
        let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("1\n2"));
        assert_matches!(e, Error::FormatNotCopyable(w) if w == want, "{what}");
        if want == "a:rPr" {
            let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b"));
            assert_matches!(e, Error::FormatNotCopyable("a:rPr"), "{what}");
        }
        p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("one line").unwrap();
    }
}

#[test]
fn copied_namespaces_must_be_in_scope_before_any_change() {
    // SYNTHETIC: an extension element in the title's a:rPr whose namespace
    // is declared only on the a:r (below the paragraph): a new run beside it
    // would have no prefix for it. Refused before any change.
    // (a direct child: an a:extLst would be refused as open content first)
    const EXT: &str = "<x14:k x14:v=\"1\"/>";
    let on_run = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        replace_once(&x, "<a:r><a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr>", &format!("<a:r xmlns:x14=\"urn:ext\"><a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/>{EXT}</a:rPr>"))
    });
    let mut p = Presentation::from_bytes(on_run).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b"));
    assert_matches!(e, Error::Core(ooxml_core::Error::NamespaceNotInScope(ref u)) if u == "urn:ext");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("a\nb"));
    assert_matches!(e, Error::Core(ooxml_core::Error::NamespaceNotInScope(_)));
    // Control: the same extension with its namespace declared on p:sld is copied,
    // with the prefix in scope at the destination.
    let on_root = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        let x = replace_once(&x, "<p:sld ", "<p:sld xmlns:x14=\"urn:ext\" ");
        replace_once(&x, "<a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr><a:t>Lorem", &format!("<a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/>{EXT}</a:rPr><a:t>Lorem"))
    });
    let mut p = Presentation::from_bytes(on_root).unwrap();
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().set_text("a\u{b}b").unwrap();
    let out = String::from_utf8(Package::from_bytes(p.to_bytes().unwrap()).unwrap().read_part("ppt/slides/slide1.xml").unwrap()).unwrap();
    assert_eq!(out.matches("<x14:k x14:v=\"1\"/>").count(), 2, "copied with the declared prefix");
}

#[test]
fn characters() {
    let mut p = open("poi/sample.pptx");
    // every setter × every forbidden character
    type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>, &str) -> Result<(), Error>;
    let ops: [(&str, Op); 5] = [
        ("run", |b, t| b.paragraph_mut(0)?.run_mut(0)?.set_text(t)),
        ("add_run", |b, t| b.paragraph_mut(0)?.add_run(t)),
        ("para", |b, t| b.paragraph_mut(0)?.set_text(t)),
        ("add_paragraph", |b, t| b.add_paragraph(t).map(|_| ())),
        ("body", |b, t| b.set_text(t)),
    ];
    for (name, op) in ops {
        for bad in ["a\u{1}", "\u{FFFE}", "x\u{0}y", "\u{1F}"] {
            let e = refused(&mut p, |p| op(&mut p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?, bad));
            assert_matches!(e, Error::InvalidXmlChar { .. }, "{name} {bad:?}");
        }
        for brk in ["a\nb", "a\rb", "a\u{b}b"] {
            let r = {
                let before = p.to_bytes().unwrap();
                let mut q = Presentation::from_bytes(before).unwrap();
                let r = op(&mut q.slide_mut(0).unwrap().shape_mut_by_id(2050).unwrap().text_body_mut().unwrap(), brk);
                r.map_err(|e| format!("{e:?}"))
            };
            let expect_ok = match name {
                "run" | "add_run" => false,
                "para" | "add_paragraph" => brk == "a\u{b}b",
                _ => true,
            };
            if expect_ok {
                assert!(r.is_ok() || r.as_ref().is_err_and(|e| e.starts_with("FormatNotCopyable")), "{name} {brk:?}: {r:?}");
            } else {
                let want = if matches!(name, "run" | "add_run") { "LineBreakInRun" } else { "LineBreakInParagraph" };
                assert!(r.as_ref().is_err_and(|e| e.starts_with(want)), "{name} {brk:?}: {r:?}");
                let e = refused(&mut p, |p| op(&mut p.slide_mut(0)?.shape_mut_by_id(2050)?.text_body_mut()?, brk));
                assert!(format!("{e:?}").starts_with(want));
            }
        }
    }
    // tab is an XML character; CR from Windows text is normalised by TextBody::set_text
    let mut s = p.slide_mut(0).unwrap();
    let mut sh = s.shape_mut_by_id(2049).unwrap();
    let mut b = sh.text_body_mut().unwrap();
    b.paragraph_mut(0).unwrap().run_mut(0).unwrap().set_text("a\tb").unwrap();
    b.paragraph_mut(0).unwrap().add_run("tail").unwrap();
    assert_eq!(b.as_ref().text(), "a\tbtail");
}

#[test]
fn animations() {
    // Real: 45545_Comment.pptx slide 1, p:bldP build="p" and p:pRg on shape 584707
    let mut p = open("poi/45545_Comment.pptx");
    let id = 584_707;
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.add_paragraph("x").map(|_| ()));
    assert_matches!(e, Error::AnimatedParagraphs { shape_id: 584_707 });
    let n = p.slide(0).unwrap().shape_by_id(id).unwrap().unwrap().text_body().unwrap().paragraphs().len();
    assert!(n > 1);
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.remove_paragraph(0));
    assert_matches!(e, Error::AnimatedParagraphs { .. });
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.set_text("one line"));
    assert_matches!(e, Error::AnimatedParagraphs { .. });
    // the same number of paragraphs is not a structure change
    let same: String = (0..n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let r = p.slide_mut(0).unwrap().shape_mut_by_id(id).unwrap().text_body_mut().unwrap().set_text(&same);
    assert!(r.is_ok() || matches!(r, Err(Error::FormatNotCopyable(_)) | Err(Error::FieldInParagraph { .. })), "{r:?}");

    // SYNTHETIC: the same slide with its p:pRg turned into p:charRg
    let chars = patch("poi/45545_Comment.pptx", "ppt/slides/slide1.xml", |x| x.replace("<p:pRg ", "<p:charRg "));
    let mut p = Presentation::from_bytes(chars).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.paragraph_mut(0)?.add_run("x"));
    assert_matches!(e, Error::AnimatedCharacters { shape_id: 584_707 });
}

#[test]
fn modern_comments() {
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    // Real: modern-comments.pptx anchors a comment to shape 2 as a whole
    // (ac:deMkLst/ac:spMk): text edits are allowed.
    let mut p = open("libreoffice/modern-comments.pptx");
    p.slide_mut(0).unwrap().shape_mut_by_id(2).unwrap().text_body_mut().unwrap().set_text("Bye").unwrap();
    // SYNTHETIC: the same anchor as a text range (ac:txMkLst with a txMk
    // after the flat monikers, the structure of the p188/ac schemas)
    let txt = patch("libreoffice/modern-comments.pptx", PART, |x| {
        replace_once(&replace_once(&x, "<ac:deMkLst ", "<ac:txMkLst "), "</ac:deMkLst>", "<ac:txMk cp=\"0\" len=\"5\"/></ac:txMkLst>")
    });
    let mut p = Presentation::from_bytes(txt).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2)?.text_body_mut()?.paragraph_mut(0)?.run_mut(0)?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 2 });
    p.slide_mut(0).unwrap().shape_mut_by_id(3).unwrap().text_body_mut().unwrap().set_text("other shape ok").unwrap();
    // SYNTHETIC: an anchor kind this crate does not know, with no ids: the whole slide
    let unknown = patch("libreoffice/modern-comments.pptx", PART, |x| {
        let a = x.find("<ac:deMkLst ").unwrap();
        let b = x.find("</ac:deMkLst>").unwrap() + "</ac:deMkLst>".len();
        format!("{}<p188:unknownAnchor/>{}", &x[..a], &x[b..])
    });
    let mut p = Presentation::from_bytes(unknown).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(3)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 3 });
    // SYNTHETIC: a comment part that is not well-formed: the whole slide
    let broken = patch("libreoffice/modern-comments.pptx", PART, |x| x.replacen("</p188:cmLst>", "", 1));
    let mut p = Presentation::from_bytes(broken).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(3)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { .. });
}

#[test]
fn namespace_not_in_scope() {
    // SYNTHETIC: sample.pptx slide 1 with the DrawingML namespace declared
    // on every a:* element instead of on p:sld (legal, unusual): a new a:p
    // under p:txBody has no prefix bound for DrawingML.
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let local = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        let x = replace_once(&x, &format!(" xmlns:a=\"{A}\""), "");
        let mut out = String::new();
        let mut rest = x.as_str();
        while let Some(i) = rest.find("<a:") {
            let name_end = rest[i + 3..].find([' ', '>', '/']).unwrap() + i + 3;
            out.push_str(&rest[..name_end]);
            out.push_str(&format!(" xmlns:a=\"{A}\""));
            rest = &rest[name_end..];
        }
        out.push_str(rest);
        out
    });
    let mut p = Presentation::from_bytes(local).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2050)?.text_body_mut()?.add_paragraph("x").map(|_| ()));
    assert_matches!(e, Error::Core(ooxml_core::Error::NamespaceNotInScope(_)));
    // inside an existing a:p the prefix is bound: run edits work
    p.slide_mut(0).unwrap().shape_mut_by_id(2050).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().run_mut(0).unwrap().set_text("ok").unwrap();
}

// ---------------------------------------------------------------------------
// Review R2-1 / R2-7: the remaining sub-conditions of §6, each on a
// synthetic input (one documented XML edit of a real fixture, in memory).

#[test]
fn strict_package() {
    // SYNTHETIC: sample.pptx with the package relationship to the main part
    // in the Strict namespace (ooxml-core refuses Strict packages)
    let strict = patch("poi/sample.pptx", "_rels/.rels", |x| {
        replace_once(
            &x,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument",
        )
    });
    assert_matches!(Presentation::from_bytes(strict), Err(Error::Core(ooxml_core::Error::StrictNotSupported(_))));
}

#[test]
fn body_without_paragraph() {
    // SYNTHETIC: sample.pptx's subtitle body with both a:p removed
    // (`<p:txBody><a:bodyPr/><a:lstStyle/></p:txBody>`, a schema violation:
    // a:p is minOccurs 1). Review R2-1: this used to panic.
    let empty = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        let at = x.find("<p:cNvPr id=\"2050\"").unwrap();
        let a = at + x[at..].find("<a:lstStyle/>").unwrap() + "<a:lstStyle/>".len();
        let b = at + x[at..].find("</p:txBody>").unwrap();
        format!("{}{}", &x[..a], &x[b..])
    });
    let mut p = Presentation::from_bytes(empty).unwrap();
    type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>;
    let ops: [Op; 4] = [
        |b| b.set_text("x"),
        |b| b.set_text("x\ny\nz"),
        |b| b.add_paragraph("y").map(|_| ()),
        |b| b.remove_paragraph(0),
    ];
    for op in ops {
        let e = refused(&mut p, |p| op(&mut p.slide_mut(0)?.shape_mut_by_id(2050)?.text_body_mut()?));
        assert_matches!(e, Error::UnsupportedContent(ref m) if m.contains("without a:p"));
    }
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2050)?.text_body_mut()?.paragraph_mut(0).map(|_| ()));
    assert_matches!(e, Error::ParagraphNotFound(0));
}

#[test]
fn content_out_of_order() {
    // SYNTHETIC: sample.pptx's title paragraph with an a:endParaRPr moved
    // before its run (`<a:p><a:pPr/><a:endParaRPr/><a:r>…</a:r></a:p>`)
    let swapped = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        let x = replace_once(&x, "<a:pPr hangingPunct=\"1\"/><a:r>", "<a:pPr hangingPunct=\"1\"/><a:endParaRPr lang=\"en-US\"/><a:r>");
        replace_once(&x, "</a:r><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp>", "</a:r></a:p></p:txBody></p:sp><p:sp>")
    });
    let mut p = Presentation::from_bytes(swapped).unwrap();
    type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>;
    let ops: [Op; 5] = [
        |b| b.paragraph_mut(0)?.run_mut(0)?.set_text("x"),
        |b| b.paragraph_mut(0)?.set_text("x"),
        |b| b.paragraph_mut(0)?.add_run("x"),
        |b| b.paragraph_mut(0)?.add_break(),
        |b| b.set_text("x"),
    ];
    for op in ops {
        let e = refused(&mut p, |p| op(&mut p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?));
        assert_matches!(e, Error::UnsupportedContent(ref m) if m.contains("out of order"));
    }
}

#[test]
fn group_without_id() {
    // SYNTHETIC: shapes.pptx slide 3 with "Group 4"'s cNvPr@id removed: its
    // members cannot be checked against references to the group.
    let noid = patch("poi/shapes.pptx", "ppt/slides/slide3.xml", |x| {
        replace_once(&x, "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"5\" name=\"Group 4\"/>", "<p:grpSp><p:nvGrpSpPr><p:cNvPr name=\"Group 4\"/>")
    });
    let mut p = Presentation::from_bytes(noid).unwrap();
    let member = path_of(&mut p, 2, |s| s.depth() > 0 && *s.kind() == ShapeKind::Shape);
    assert!(p.slide(2).unwrap().shapes().iter().any(|s| s.path() == member && s.id().is_ok()), "the member's own id is valid");
    let e = refused(&mut p, |p| p.slide_mut(2)?.shape_mut(&member).map(|_| ()));
    assert_matches!(e, Error::InvalidShapeId);
}

#[test]
fn animation_sub_conditions() {
    const ID: u32 = 584_707;
    let other = |p: &mut Presentation| {
        let s = p.slide(0).unwrap();
        s.shapes().into_iter().find(|x| x.text_body().is_some() && x.id().is_ok_and(|i| i != ID)).unwrap().id().unwrap()
    };
    // SYNTHETIC: 45545_Comment.pptx with every p:txEl/p:pRg renamed to an
    // unknown child: offsets this crate cannot interpret lock all text edits
    let unknown = patch("poi/45545_Comment.pptx", "ppt/slides/slide1.xml", |x| x.replace("<p:pRg ", "<p:futureRg "));
    let mut p = Presentation::from_bytes(unknown).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(ID)?.text_body_mut()?.paragraph_mut(0)?.add_run("x"));
    assert_matches!(e, Error::AnimatedCharacters { shape_id: ID });
    // SYNTHETIC: the same slide with the bldP's spid not a number: every
    // shape's paragraph structure is locked, not only shape 584707's
    let nan = patch("poi/45545_Comment.pptx", "ppt/slides/slide1.xml", |x| {
        replace_once(&x, "<p:bldP spid=\"584707\"", "<p:bldP spid=\"x584707\"")
    });
    let mut p = Presentation::from_bytes(nan).unwrap();
    let o = other(&mut p);
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(o)?.text_body_mut()?.add_paragraph("x").map(|_| ()));
    assert_matches!(e, Error::AnimatedParagraphs { shape_id } if shape_id == o);
    // ... while the unmodified fixture lets that shape grow
    let mut p = open("poi/45545_Comment.pptx");
    p.slide_mut(0).unwrap().shape_mut_by_id(o).unwrap().text_body_mut().unwrap().add_paragraph("x").unwrap();
}

#[test]
fn comment_sub_conditions() {
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    const RELS: &str = "ppt/slides/_rels/slide1.xml.rels";
    // SYNTHETIC: the comment relationship points to a part that does not exist
    let missing = patch("libreoffice/modern-comments.pptx", RELS, |x| {
        replace_once(&x, "../comments/modernComment_100_68C49C6.xml", "../comments/missing.xml")
    });
    let mut p = Presentation::from_bytes(missing).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(3)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 3 });
    // SYNTHETIC: the p188:cm without any anchor (its ac:deMkLst removed)
    let anchorless = patch("libreoffice/modern-comments.pptx", PART, |x| {
        let a = x.find("<ac:deMkLst ").unwrap();
        let b = x.find("</ac:deMkLst>").unwrap() + "</ac:deMkLst>".len();
        format!("{}{}", &x[..a], &x[b..])
    });
    let mut p = Presentation::from_bytes(anchorless).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(3)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 3 });
}

#[test]
fn comment_and_id_sub_conditions_round3() {
    // Review N3-2: the remaining §6 sub-conditions, each on a synthetic input.
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    // SYNTHETIC: an unknown child of p188:cmLst: the whole slide is locked
    let unknown_child = patch("libreoffice/modern-comments.pptx", PART, |x| replace_once(&x, "</p188:cmLst>", "<p188:futureThing/></p188:cmLst>"));
    let mut p = Presentation::from_bytes(unknown_child).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(3)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 3 });
    // SYNTHETIC: ac:deMkLst without a txMk element but with cp/len offsets
    // on its shape moniker: not a whole-drawing anchor; shape 2 is locked,
    // shape 3 is not
    let offsets = patch("libreoffice/modern-comments.pptx", PART, |x| {
        let at = x.find("<ac:spMk ").unwrap();
        format!("{}<ac:spMk cp=\"0\" len=\"3\" {}", &x[..at], &x[at + "<ac:spMk ".len()..])
    });
    let mut p = Presentation::from_bytes(offsets).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 2 });
    p.slide_mut(0).unwrap().shape_mut_by_id(3).unwrap().text_body_mut().unwrap().set_text("other shape ok").unwrap();
    // SYNTHETIC: sample.pptx's title with cNvPr@id not a number (present,
    // unlike shape_addressing's missing id)
    let nan = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| replace_once(&x, "id=\"2049\"", "id=\"title\""));
    let mut p = Presentation::from_bytes(nan).unwrap();
    let path = path_of(&mut p, 0, |s| s.placeholder().is_some());
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut(&path).map(|_| ()));
    assert_matches!(e, Error::InvalidShapeId);
}

#[test]
fn unreadable_comment_parts_fail_closed() {
    // PR #16 review m4: any failure to read or parse a comment part locks
    // the text of every shape (it never disables the guard). SYNTHETIC
    // comment parts: not XML at all, empty, not UTF-8, and nested deeper
    // than any real part (each element level an unknown anchor wrapper).
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    let deep = {
        let mut s = String::from(r#"<p188:cmLst xmlns:p188="http://schemas.microsoft.com/office/powerpoint/2018/8/main"><p188:cm id="x">"#);
        s.push_str(&"<p188:n>".repeat(1500));
        s.push_str(&"</p188:n>".repeat(1500));
        s.push_str("</p188:cm></p188:cmLst>");
        s.into_bytes()
    };
    for (what, bytes) in [("binary", vec![0xFF, 0xFE, 0x00, 0x01]), ("empty", Vec::new()), ("latin-1", b"<p188:cmLst \xE9/>".to_vec()), ("deep", deep)] {
        let mut pkg = Package::from_bytes(cases::read_fixture("libreoffice/modern-comments.pptx")).unwrap();
        pkg.set_part(PART, bytes).unwrap();
        let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(3)?.text_body_mut()?.set_text("x"));
        assert_matches!(e, Error::CommentAnchoredText { shape_id: 3 }, "{what}");
    }
}

#[test]
fn too_deep_comment_part_fails_closed() {
    // PR #16 (depth limit 256) + its review m4: a comment part nested
    // deeper than the limit does not parse (`TooDeep`), and the slide's text
    // is then locked. SYNTHETIC: the real anchor of modern-comments.pptx is
    // a whole-shape `ac:deMkLst` (allowed); 300 extra levels inside it make
    // the part unparsable, and only that turns the edit into a refusal.
    const PART: &str = "ppt/comments/modernComment_100_68C49C6.xml";
    let deep = patch("libreoffice/modern-comments.pptx", PART, |x| {
        replace_once(&x, "</ac:deMkLst>", &format!("{}{}</ac:deMkLst>", "<ac:x>".repeat(300), "</ac:x>".repeat(300)))
    });
    let part = Package::from_bytes(deep.clone()).unwrap().read_part(PART).unwrap();
    assert_matches!(ooxml_core::XmlDocument::parse(&part), Err(ooxml_core::Error::TooDeep { limit: 256, .. }));
    let mut p = Presentation::from_bytes(deep).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2)?.text_body_mut()?.set_text("x"));
    assert_matches!(e, Error::CommentAnchoredText { shape_id: 2 });
    // control: 200 extra levels parse, and the whole-shape anchor allows the edit
    let ok = patch("libreoffice/modern-comments.pptx", PART, |x| {
        replace_once(&x, "</ac:deMkLst>", &format!("{}{}</ac:deMkLst>", "<ac:x>".repeat(200), "</ac:x>".repeat(200)))
    });
    let mut p = Presentation::from_bytes(ok).unwrap();
    p.slide_mut(0).unwrap().shape_mut_by_id(2).unwrap().text_body_mut().unwrap().set_text("x").unwrap();
}

#[test]
fn edits_that_would_pass_the_depth_limit_are_refused_before_any_change() {
    // PR #16: the lens refuses inserts beyond the depth limit (256) only at
    // the insert that crosses it. A text edit makes several inserts, so the
    // setter checks the deepest one first. SYNTHETIC: a slide whose text
    // body sits 248 groups deep, with an empty paragraph whose a:endParaRPr
    // has a child at depth 256 (the limit). Filling it creates a:r (255),
    // a:rPr (256) and the copied a:latin (257): refused, nothing changed.
    const N: usize = 248;
    let mut x = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
        r#"<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#
    ));
    for k in 0..N {
        x.push_str(&format!(r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{}" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#, 10 + k));
    }
    x.push_str(r#"<p:sp><p:nvSpPr><p:cNvPr id="5" name="t"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:endParaRPr lang="en-US"><a:latin typeface="X"/></a:endParaRPr></a:p></p:txBody></p:sp>"#);
    x.push_str(&"</p:grpSp>".repeat(N));
    x.push_str("</p:spTree></p:cSld></p:sld>");
    let deep = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |_| x.clone());
    let mut p = Presentation::from_bytes(deep).unwrap();
    type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>;
    let ops: [Op; 3] = [|b| b.paragraph_mut(0)?.set_text("x"), |b| b.set_text("x"), |b| b.set_text("a\nb")];
    for op in ops {
        let e = refused(&mut p, |p| op(&mut p.slide_mut(0)?.shape_mut_by_id(5)?.text_body_mut()?));
        assert_matches!(e, Error::Core(ooxml_core::Error::TooDeep { limit: 256, .. }));
    }
    // unformatted text fits: a:r 255, a:t 256
    p.slide_mut(0).unwrap().shape_mut_by_id(5).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().add_run("fits").unwrap();
    let out = p.to_bytes().unwrap();
    let part = Package::from_bytes(out).unwrap().read_part("ppt/slides/slide1.xml").unwrap();
    ooxml_core::XmlDocument::parse(&part).expect("the edited part parses again under the default limit");
}

// ---------------------------------------------------------------------------
// Review #19 round 1 (N1, N2, N4) and #18 round 2: what Phase 0.1 copies.

const TITLE_RPR: &str = "<a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr><a:t>Lorem";
const TITLE_END: &str = "</a:r><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp><p:sp>";

fn sample_with(from: &str, to: &str) -> Presentation {
    Presentation::from_bytes(patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| replace_once(&x, from, to))).unwrap()
}

fn title(p: &mut Presentation) -> Vec<RawNode> {
    body_of(&p.slide(0).unwrap(), 2049).iter().map(|q| q.raw().clone()).collect()
}

#[test]
fn smart_tag_ids_and_extension_lists_are_not_copied() {
    // review #19 round 2 R2-1 / #18 R3-1: a named effect container
    // ("Shall be unique across all effect trees and effect containers",
    // ECMA-376 Part 1 §20.1.8.25). SYNTHETIC: on the title's a:rPr
    let mut p = sample_with(TITLE_RPR, "<a:rPr lang=\"en-US\"><a:effectDag name=\"E\" type=\"sib\"/></a:rPr><a:t>Lorem");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b"));
    assert_matches!(e, Error::FormatNotCopyable("a:rPr"));
    // N1 / #18 R2-1: smtId is "unique throughout the presentation"
    // (ECMA-376 Part 1 §21.1.2.3.9); a copy would duplicate it.
    // SYNTHETIC: the title's a:rPr with smtId="4242"
    let mut p = sample_with(TITLE_RPR, "<a:rPr lang=\"en-US\" smtId=\"4242\"/><a:t>Lorem");
    type Op = fn(&mut ooxml_pptx::TextBodyMut<'_>) -> Result<(), Error>;
    let ops: [Op; 3] = [|b| b.paragraph_mut(0)?.set_text("a\u{b}b"), |b| b.set_text("a\nb"), |b| b.set_text("a\nb\nc")];
    for op in ops {
        let e = refused(&mut p, |p| op(&mut p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?));
        assert_matches!(e, Error::FormatNotCopyable("a:rPr"));
    }
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("one line keeps the run").unwrap();
    // smtId="0" is the default, not an id: copied
    let mut p = sample_with(TITLE_RPR, "<a:rPr lang=\"en-US\" smtId=\"0\"/><a:t>Lorem");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().set_text("a\u{b}b").unwrap();
    // SYNTHETIC: an a:defRPr with smtId inside the title's a:pPr
    let mut p = sample_with("<a:pPr hangingPunct=\"1\"/><a:r>", "<a:pPr hangingPunct=\"1\"><a:defRPr smtId=\"3\"/></a:pPr><a:r>");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("1\n2"));
    assert_matches!(e, Error::FormatNotCopyable("a:pPr"));
    // SYNTHETIC: an extension list in the title's a:rPr (#18 R2-2)
    let mut p = sample_with(TITLE_RPR, "<a:rPr lang=\"en-US\"><a:extLst><a:ext uri=\"{X}\"/></a:extLst></a:rPr><a:t>Lorem");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b"));
    assert_matches!(e, Error::FormatNotCopyable("a:rPr"));
}

#[test]
fn ancestor_ignorable_namespaces_are_seen_through_the_slide() {
    // N4 (P10): the copy is detached with a lens that knows its ancestors
    // (Lens::at), so mc:Ignorable on p:sld counts. SYNTHETIC: p:sld makes
    // urn:ext ignorable and the title's a:rPr uses it.
    let bytes = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        let x = replace_once(&x, "<p:sld ", "<p:sld xmlns:e=\"urn:ext\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"e\" ");
        replace_once(&x, TITLE_RPR, "<a:rPr lang=\"en-US\" e:x=\"1\"/><a:t>Lorem")
    });
    let mut p = Presentation::from_bytes(bytes).unwrap();
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b"));
    assert_matches!(e, Error::FormatNotCopyable("a:rPr"));
}

#[test]
fn end_paragraph_format_sources() {
    // N4 (P4, P5, P8, P9, P11), each SYNTHETIC on the title (one paragraph,
    // first run a:rPr with a:sym).
    let rpr_of = |n: &RawNode| child(child(n, "r").unwrap(), "rPr").map(|r| subtree(r, &[]));
    let end_of = |n: &RawNode| child(n, "endParaRPr").map(|r| subtree(r, &[]));
    let sym = rpr_of(&title(&mut open("poi/sample.pptx"))[0]).unwrap();
    // own a:endParaRPr with child elements only (no attributes): it is a format
    let mut p = sample_with(TITLE_END, "</a:r><a:endParaRPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></a:endParaRPr></a:p></p:txBody></p:sp><p:sp>");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("a\nb").unwrap();
    assert!(end_of(&title(&mut p)[1]).unwrap().contains("solidFill"), "own end format with children only");
    // own a:endParaRPr with only a link: not a format, the run source is used
    let mut p = sample_with(TITLE_END, "</a:r><a:endParaRPr><a:hlinkClick r:id=\"rId1\"/></a:endParaRPr></a:p></p:txBody></p:sp><p:sp>");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("a\nb").unwrap();
    assert_eq!(end_of(&title(&mut p)[1]), Some(sym.clone()), "link-only end falls back to the run format");
    // own a:endParaRPr with a format and a link: copied without the link
    let mut p = sample_with(TITLE_END, "</a:r><a:endParaRPr lang=\"de-DE\"><a:hlinkClick r:id=\"rId1\"/></a:endParaRPr></a:p></p:txBody></p:sp><p:sp>");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("a\nb").unwrap();
    let end = end_of(&title(&mut p)[1]).unwrap();
    assert!(end.contains("de-DE") && !end.contains("hlinkClick"), "{end}");
    // an empty first a:rPr: new runs get no a:rPr at all
    let mut p = sample_with(TITLE_RPR, "<a:rPr/><a:t>Lorem");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().set_text("a\u{b}b").unwrap();
    let t = title(&mut p);
    let runs: Vec<&RawNode> = t[0].elements().filter(|c| c.name().local() == "r").collect();
    assert_eq!(runs.len(), 2);
    assert!(child(runs[1], "rPr").is_none(), "no empty a:rPr is created");
    // an empty paragraph whose a:endParaRPr (the run source) has a link:
    // the new run's a:rPr has its format but not the link. The subtitle's
    // second paragraph is empty.
    let bytes = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        replace_once(&x, "<a:p><a:pPr hangingPunct=\"1\"/><a:endParaRPr lang=\"en-US\"/></a:p>", "<a:p><a:pPr hangingPunct=\"1\"/><a:endParaRPr lang=\"en-US\" b=\"1\"><a:hlinkClick r:id=\"rId1\"/></a:endParaRPr></a:p>")
    });
    let mut p = Presentation::from_bytes(bytes).unwrap();
    p.slide_mut(0).unwrap().shape_mut_by_id(2050).unwrap().text_body_mut().unwrap().paragraph_mut(1).unwrap().set_text("x").unwrap();
    let q = body_of(&p.slide(0).unwrap(), 2050)[1].raw().clone();
    let r = rpr_of(&q).unwrap();
    assert!(r.contains("\"1\"") && !r.contains("hlinkClick"), "{r}");
    assert!(end_of(&q).unwrap().contains("hlinkClick"), "the paragraph's own a:endParaRPr is untouched");
}

#[test]
fn an_unused_run_source_is_not_consulted() {
    // N3: a new *empty* paragraph needs only a:pPr and a:endParaRPr; with the
    // last paragraph's own end format, the uncopyable run source (picture
    // fill with r:embed) does not matter. SYNTHETIC on the title.
    let mut p = sample_with(TITLE_RPR, "<a:rPr lang=\"en-US\"><a:blipFill><a:blip r:embed=\"rId1\"/></a:blipFill></a:rPr><a:t>Lorem");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("x\n").unwrap();
    // ... but a new line with text needs it
    let mut p = sample_with(TITLE_RPR, "<a:rPr lang=\"en-US\"><a:blipFill><a:blip r:embed=\"rId1\"/></a:blipFill></a:rPr><a:t>Lorem");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("x\ny"));
    assert_matches!(e, Error::FormatNotCopyable("a:rPr"));
}

/// sample.pptx slide 1 with the title shape nested `k` groups deep and its
/// body replaced by `body`.
fn nested_title(k: usize, body: &str) -> Presentation {
    let bytes = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |x| {
        let i = x.find("id=\"2049\"").unwrap();
        let s = x[..i].rfind("<p:sp>").unwrap();
        let e = x[i..].find("</p:sp>").unwrap() + i + "</p:sp>".len();
        let sp = &x[s..e];
        let (bs, be) = (sp.find("<p:txBody>").unwrap(), sp.find("</p:txBody>").unwrap() + "</p:txBody>".len());
        let sp = format!("{}<p:txBody><a:bodyPr/>{body}</p:txBody>{}", &sp[..bs], &sp[be..]);
        let (mut open, mut close) = (String::new(), String::new());
        for g in 0..k {
            open += &format!("<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"{}\" name=\"g\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>", 5000 + g);
            close += "</p:grpSp>";
        }
        format!("{}{open}{sp}{close}{}", &x[..s], &x[e..])
    });
    Presentation::from_bytes(bytes).unwrap()
}

#[test]
fn depth_pre_checks_decide_alone() {
    // N2: inputs where each pre-check alone stands between the edit and a
    // half-applied change (k groups: p:sp at depth 4+k, p:txBody 5+k, a:p 6+k).
    let reparses = |p: &mut Presentation| {
        let out = p.to_bytes().unwrap();
        let part = Package::from_bytes(out).unwrap().read_part("ppt/slides/slide1.xml").unwrap();
        ooxml_core::XmlDocument::parse(&part).is_ok()
    };
    // Paragraph::set_text into a bare <a:p/> (no format source): a:r 256,
    // a:t 257 at k = 249: only the paragraph depth check refuses it
    let mut p = nested_title(249, "<a:p/>");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("x"));
    assert_matches!(e, Error::Core(ooxml_core::Error::TooDeep { limit: 256, .. }));
    let mut p = nested_title(248, "<a:p/>");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().paragraph_mut(0).unwrap().set_text("x").unwrap();
    assert!(reparses(&mut p));
    // TextBody::set_text("\nx") on a bare <a:p/> at k = 249: line 0 creates
    // nothing in the existing paragraph; the new paragraph's a:t (257) is
    // refused by the body's own depth check alone (no format to copy)
    let mut p = nested_title(249, "<a:p/>");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("\nx"));
    assert_matches!(e, Error::Core(ooxml_core::Error::TooDeep { .. }));
    let mut p = nested_title(249, "<a:p/>");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("").unwrap();
    // add_paragraph at k = 249: a:p 255, a:r 256, a:t 257
    let mut p = nested_title(249, "<a:p/>");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.add_paragraph("x").map(|_| ()));
    assert_matches!(e, Error::Core(ooxml_core::Error::TooDeep { .. }));
    let mut p = nested_title(248, "<a:p/>");
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().add_paragraph("x").unwrap();
    assert!(reparses(&mut p));
    // TextBody::set_text("\n\ny") on an empty paragraph whose a:endParaRPr
    // (solidFill/srgbClr, 3 levels) becomes the new run's a:rPr two levels
    // below the body: at k = 247 the copy reaches 257; the a:endParaRPr
    // copies (one level below the body) still fit, so only the check with
    // two created levels refuses it
    let end = "<a:p><a:endParaRPr lang=\"en-US\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></a:endParaRPr></a:p>";
    let mut p = nested_title(247, end);
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("\n\ny"));
    assert_matches!(e, Error::Core(ooxml_core::Error::TooDeep { .. }));
    let mut p = nested_title(246, end);
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("\n\ny").unwrap();
    assert!(reparses(&mut p));
}

// ---------------------------------------------------------------------------
// Review #19 round 2 (R2-2, R2-3, R2-4).

#[test]
fn add_run_depth_and_body_copy_checks_decide_alone() {
    // R2-2 (D6): add_run into a bare <a:p/> at depth 255 (k = 249): a:r 256,
    // a:t 257; only add_run's own depth check prevents a half-applied edit
    let mut p = nested_title(249, "<a:p/>");
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.add_run("x"));
    assert_matches!(e, Error::Core(ooxml_core::Error::TooDeep { .. }));
    // R2-2 (P7b/c): the title's a:p declares urn:ext and its a:pPr uses it;
    // a new paragraph goes under the body, where urn:ext is not bound: the
    // body-level copy check refuses before the new a:p is inserted
    let mut p = sample_with(
        "<a:p><a:pPr hangingPunct=\"1\"/><a:r>",
        "<a:p xmlns:x14=\"urn:ext\"><a:pPr hangingPunct=\"1\" x14:k=\"1\"/><a:r>",
    );
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.set_text("a\nb"));
    assert_matches!(e, Error::Core(ooxml_core::Error::NamespaceNotInScope(ref u)) if u == "urn:ext");
}

#[test]
fn a_new_line_starting_with_a_break_still_gets_the_run_format() {
    // R2-3 (N3d): in "x\n\u{b}b" the new paragraph's first segment is empty;
    // its run "b" must still copy the format (the title's a:rPr with a:sym)
    let mut p = open("poi/sample.pptx");
    let rpr = subtree(child(child(&title(&mut p)[0], "r").unwrap(), "rPr").unwrap(), &[]);
    p.slide_mut(0).unwrap().shape_mut_by_id(2049).unwrap().text_body_mut().unwrap().set_text("x\n\u{b}b").unwrap();
    let t = title(&mut p);
    let run = child(&t[1], "r").expect("the run of \"b\"");
    assert_eq!(subtree(child(run, "rPr").expect("formatted"), &[]), rpr);
}

#[test]
fn destination_refusals_of_a_copy_are_format_not_copyable() {
    // R2-4: the title paragraph is xml:space="preserve", its first run resets
    // it to "default", and the run's a:rPr has whitespace text: detaching is
    // fine (the run's own default), inserting a new run under the preserve
    // paragraph would change the whitespace's meaning
    let mut p = sample_with(
        "<a:p><a:pPr hangingPunct=\"1\"/><a:r><a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr>",
        "<a:p xml:space=\"preserve\"><a:pPr hangingPunct=\"1\"/><a:r xml:space=\"default\"><a:rPr lang=\"en-US\">\n  <a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/>\n</a:rPr>",
    );
    let e = refused(&mut p, |p| p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\u{b}b"));
    assert_matches!(e, Error::FormatNotCopyable("a:rPr"));
}

#[test]
fn paragraph_insertion_preserves_text_body_with_same_slot_alternate_content() {
    let ac = format!(r#"<mc:AlternateContent xmlns:mc="{}"><mc:Choice Requires="a"><a:p/></mc:Choice></mc:AlternateContent>"#,
                     ooxml_core::name::MC_NS);
    let src = patch("poi/sample.pptx", "ppt/slides/slide1.xml", |xml| xml.replacen("</p:txBody>", &format!("{ac}</p:txBody>"), 1));
    let mut p = Presentation::from_bytes(src).unwrap();
    assert_matches!(
        refused(&mut p, |p| {
            p.slide_mut(0)?.shape_mut_by_id(2049)?.text_body_mut()?.add_paragraph("new").map(|_| ())
        }),
        Error::UnsupportedContent(_)
    );
}
