//! Phase 1b SmartArt text (ADR-0013 §4) on `poi/SmartArt.pptx` (PowerPoint
//! output, five placeholder nodes, a drawing cache): edits reach the data
//! model and the cache slice, only those two parts change, placeholders
//! lose `phldr`, and every refusal leaves the package byte-identical.

use std::assert_matches;
use std::io::{Cursor, Read, Write};

use ooxml_pptx::{Error, Presentation};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx/poi/SmartArt.pptx");
const DATA: &str = "ppt/diagrams/data1.xml";
const DRAWING: &str = "ppt/diagrams/drawing1.xml";
const NODE: &str = "{EC0D1448-4AB9-47C0-A4EF-4E8640283403}";
/// The drawing shape that shows `NODE` (its presOf destination).
const SHAPE: &str = "{6B75AEA0-469E-42BA-9B81-3A0807E17FCA}";

fn src() -> Vec<u8> {
    std::fs::read(FIX).unwrap()
}

fn part(b: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

fn has(b: &[u8], name: &str) -> bool {
    zip::ZipArchive::new(Cursor::new(b)).unwrap().by_name(name).is_ok()
}

fn raw_entries(b: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index_raw(i).unwrap();
            let mut v = Vec::new();
            f.read_to_end(&mut v).unwrap();
            (f.name().unwrap().into_owned(), f.crc32(), v)
        })
        .collect()
}

fn patch(b: &[u8], name: &str, f: impl Fn(String) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    let mut seen = false;
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let n = e.name().unwrap().into_owned();
        let mut s = Vec::new();
        e.read_to_end(&mut s).unwrap();
        if n == name {
            seen = true;
            s = f(String::from_utf8(s).unwrap()).into_bytes();
        }
        w.start_file(n, opts).unwrap();
        w.write_all(&s).unwrap();
    }
    if !seen {
        w.start_file(name, opts).unwrap();
        w.write_all(f(String::new()).as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn unchanged(b: &[u8]) -> Vec<u8> {
    Presentation::from_bytes(b.to_vec()).unwrap().to_bytes().unwrap()
}

/// The `dsp:sp` with this modelId, as XML.
fn shape_xml(drawing: &str, model: &str) -> String {
    let i = drawing.find(&format!("<dsp:sp modelId=\"{model}\"")).unwrap();
    let j = i + drawing[i..].find("</dsp:sp>").unwrap();
    drawing[i..j].to_owned()
}

/// The `dgm:pt` with this modelId, as XML.
fn point_xml(data: &str, model: &str) -> String {
    let i = data.find(&format!("<dgm:pt modelId=\"{model}\"")).unwrap();
    let j = i + data[i..].find("</dgm:pt>").unwrap();
    data[i..j].to_owned()
}

fn texts(xml: &str) -> Vec<String> {
    xml.split("<a:t>").skip(1).map(|s| s.split("</a:t>").next().unwrap().to_owned()).collect()
}

fn edit(b: &[u8], f: impl FnOnce(&mut ooxml_pptx::SmartArtMut<'_>) -> Result<(), Error>) -> (Result<(), Error>, Vec<u8>) {
    let mut p = Presentation::from_bytes(b.to_vec()).unwrap();
    let r = f(&mut p.smart_art_mut(0, 4).unwrap());
    (r, p.to_bytes().unwrap())
}

#[test]
fn set_text_reaches_data_and_cache_only() {
    let s = src();
    let base = unchanged(&s);
    let (r, out) = edit(&s, |sa| sa.set_text(NODE, "First line\nSecond line"));
    r.unwrap();
    // only the data and the drawing part change
    let a = raw_entries(&base);
    let b = raw_entries(&out);
    for (n, crc, v) in &a {
        if n == DATA || n == DRAWING {
            continue;
        }
        let w = b.iter().find(|x| &x.0 == n).unwrap();
        assert_eq!((crc, v), (&w.1, &w.2), "{n} changed");
    }
    let pt = point_xml(&part(&out, DATA), NODE);
    assert_eq!(texts(&pt), ["First line", "Second line"]);
    assert!(!pt.contains("phldr=\"1\"") && pt.contains("phldrT=\"[Text]\""), "phldr removed, phldrT kept: {pt}");
    let sp = shape_xml(&part(&out, DRAWING), SHAPE);
    assert_eq!(texts(&sp), ["First line", "Second line"]);
    // the new cache paragraph copies the slice's paragraph format
    assert_eq!(sp.matches("<a:pPr marL=\"0\" lvl=\"0\" indent=\"0\" algn=\"ctr\"").count(), 2, "{sp}");
    // other shapes are untouched
    let d0 = part(&s, DRAWING);
    let d1 = part(&out, DRAWING);
    assert_eq!(d0.replace(&shape_xml(&d0, SHAPE), ""), d1.replace(&shape_xml(&d1, SHAPE), ""));
    // the reopened deck reads the new text and the cache still pairs
    let mut q = Presentation::from_bytes(out).unwrap();
    let mut sa = q.smart_art_mut(0, 4).unwrap();
    let n = sa.nodes().unwrap().into_iter().find(|n| n.model_id == NODE).unwrap();
    assert_eq!(n.paragraphs, ["First line", "Second line"]);
    let rep = sa.cache_report().unwrap();
    assert_eq!((rep.nodes, rep.consistent), (5, 5));
}

#[test]
fn paragraph_and_run_edits_pair_by_runs_with_text() {
    let s = src();
    let mut p = Presentation::from_bytes(s).unwrap();
    {
        let mut sa = p.smart_art_mut(0, 4).unwrap();
        sa.set_text(NODE, "alpha").unwrap();
        sa.set_paragraph_text(NODE, 0, "beta").unwrap();
        sa.set_run_text(NODE, 0, 0, "gamma").unwrap();
        // emptying the run, then filling it again: both sides ignore it while empty
        sa.set_run_text(NODE, 0, 0, "").unwrap();
        assert_eq!(sa.cache_report().unwrap().consistent, 5);
        sa.set_run_text(NODE, 0, 0, "delta").unwrap();
    }
    let out = p.to_bytes().unwrap();
    assert_eq!(texts(&point_xml(&part(&out, DATA), NODE)), ["delta"]);
    let sp = shape_xml(&part(&out, DRAWING), SHAPE);
    let t = texts(&sp);
    assert_eq!(t.iter().filter(|x| !x.is_empty()).collect::<Vec<_>>(), ["delta"], "{sp}");
    let mut q = Presentation::from_bytes(out).unwrap();
    assert_eq!(q.smart_art_mut(0, 4).unwrap().cache_report().unwrap().consistent, 5);
}

#[test]
fn a_node_without_dgm_t_gets_one() {
    let s = patch(&src(), DATA, |d| {
        let pt = point_xml(&d, NODE);
        let i = pt.find("<dgm:t>").unwrap();
        let j = pt.find("</dgm:t>").unwrap() + 8;
        d.replace(&pt, &format!("{}{}", &pt[..i], &pt[j..]))
    });
    let (r, out) = edit(&s, |sa| sa.set_text(NODE, "made"));
    r.unwrap();
    let pt = point_xml(&part(&out, DATA), NODE);
    assert!(pt.contains("<dgm:t><a:bodyPr/><a:lstStyle/><a:p>"), "{pt}");
    assert_eq!(texts(&pt), ["made"]);
    assert_eq!(texts(&shape_xml(&part(&out, DRAWING), SHAPE)), ["made"]);
}

#[test]
fn refusals_leave_the_package_unchanged() {
    let s = src();
    // an unknown node
    let (r, out) = edit(&s, |sa| sa.set_text("{nope}", "x"));
    assert_matches!(r, Err(Error::SmartArtNodeNotFound(_)));
    assert_eq!(out, unchanged(&s));
    // a cache whose text does not pair with the data
    let stale = patch(&s, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        let fixed = sp.replacen("<a:endParaRPr", "<a:r><a:rPr lang=\"en-US\"/><a:t>stale</a:t></a:r><a:endParaRPr", 1);
        d.replace(&sp, &fixed)
    });
    let (r, out) = edit(&stale, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(_)));
    assert_eq!(out, unchanged(&stale));
    // two sources sharing a destOrd
    let tie = patch(&s, DATA, |d| {
        d.replacen("type=\"presOf\" srcId=\"{B6892F9D-F666-456D-A4A6-2293F5DE4029}\" destId=\"{B2C0AC72-CE0D-41E8-8E4E-EB1B9DAEAD34}\"", "type=\"presOf\" srcId=\"{B6892F9D-F666-456D-A4A6-2293F5DE4029}\" destId=\"{6B75AEA0-469E-42BA-9B81-3A0807E17FCA}\"", 1)
    });
    let (r, out) = edit(&tie, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(ref m)) if m.contains("destOrd"));
    assert_eq!(out, unchanged(&tie));
    // a cache reference that does not resolve from the slide
    let dangling = patch(&s, DATA, |d| d.replace("relId=\"rId6\"", "relId=\"rId99\""));
    let (r, out) = edit(&dangling, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheUnresolved));
    assert_eq!(out, unchanged(&dangling));
    // a hyperlink run cannot be mirrored (its r:id differs between the parts)
    let link = |d: String, rid: &str| {
        let pt = point_xml(&d, NODE);
        d.replace(&pt, &pt.replacen("<a:p><a:endParaRPr", &format!("<a:p><a:r><a:rPr lang=\"en-US\"><a:hlinkClick xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"{rid}\"/></a:rPr><a:t>L</a:t></a:r><a:endParaRPr"), 1))
    };
    let linked = patch(&s, DATA, |d| link(d, "rId1"));
    let linked = patch(&linked, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        d.replace(&sp, &sp.replacen("<a:endParaRPr", "<a:r><a:rPr lang=\"en-US\"/><a:t>L</a:t></a:r><a:endParaRPr", 1))
    });
    let (r, out) = edit(&linked, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::FormatNotCopyable("a:rPr")));
    assert_eq!(out, unchanged(&linked));
}

#[test]
fn comment_anchors_on_the_frame_lock_its_text() {
    let s = src();
    let cm = "<p188:cmLst xmlns:p188=\"http://schemas.microsoft.com/office/powerpoint/2018/8/main\" xmlns:ac=\"http://schemas.microsoft.com/office/drawing/2013/main/command\"><p188:cm id=\"{1}\" authorId=\"{2}\"><ac:txMkLst><ac:graphicFrameMk id=\"4\" creationId=\"{0}\"/><ac:txMk cp=\"0\" len=\"1\"/></ac:txMkLst></p188:cm></p188:cmLst>";
    let b = patch(&s, "ppt/comments/modernComment_1.xml", |_| cm.to_owned());
    let b = patch(&b, "ppt/slides/_rels/slide1.xml.rels", |r| {
        r.replace("</Relationships>", "<Relationship Id=\"rId90\" Type=\"http://schemas.microsoft.com/office/2018/10/relationships/comments\" Target=\"../comments/modernComment_1.xml\"/></Relationships>")
    });
    let b = patch(&b, "[Content_Types].xml", |c| {
        c.replace("</Types>", "<Override PartName=\"/ppt/comments/modernComment_1.xml\" ContentType=\"application/vnd.ms-powerpoint.comments+xml\"/></Types>")
    });
    let (r, out) = edit(&b, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::CommentAnchoredText { shape_id: 4 }));
    assert_eq!(out, unchanged(&b));
}

#[test]
fn drop_drawing_cache_removes_the_cache_and_its_reference() {
    let s = src();
    let (r, out) = edit(&s, |sa| sa.drop_drawing_cache());
    r.unwrap();
    assert!(!has(&out, DRAWING) && !has(&out, "ppt/diagrams/_rels/drawing1.xml.rels"));
    assert!(!part(&out, DATA).contains("dataModelExt"));
    assert!(!part(&out, "ppt/slides/_rels/slide1.xml.rels").contains("diagramDrawing"));
    assert!(!part(&out, "[Content_Types].xml").contains("/ppt/diagrams/drawing1.xml"));
    // edits now reach the data only
    let mut p = Presentation::from_bytes(out).unwrap();
    p.smart_art_mut(0, 4).unwrap().set_text(NODE, "data only").unwrap();
    let out = p.to_bytes().unwrap();
    assert_eq!(texts(&point_xml(&part(&out, DATA), NODE)), ["data only"]);
    // a drawing another part also references is not dropped
    let shared = patch(&s, "ppt/slideLayouts/_rels/slideLayout1.xml.rels", |r| {
        r.replace("</Relationships>", "<Relationship Id=\"rId77\" Type=\"http://schemas.microsoft.com/office/2007/relationships/diagramDrawing\" Target=\"../diagrams/drawing1.xml\"/></Relationships>")
    });
    let (r, out) = edit(&shared, |sa| sa.drop_drawing_cache());
    assert_matches!(r, Err(Error::SmartArtCacheShared));
    assert_eq!(out, unchanged(&shared));
    // an unreadable relationships part anywhere fails closed (core)
    let broken = patch(&s, "ppt/slideLayouts/_rels/slideLayout2.xml.rels", |_| "<not".into());
    let (r, out) = edit(&broken, |sa| sa.drop_drawing_cache());
    assert_matches!(r, Err(Error::Core(_)));
    assert_eq!(out, unchanged(&broken));
}

#[test]
fn not_smart_art() {
    let s = src();
    let mut p = Presentation::from_bytes(s).unwrap();
    assert_matches!(p.smart_art_mut(0, 99), Err(Error::ShapeNotFound));
    let t = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx/poi/table_test2.pptx")).unwrap();
    let mut p = Presentation::from_bytes(t).unwrap();
    assert_matches!(p.smart_art_mut(0, 4), Err(Error::NotSmartArt));
}

/// `NODE` with text "alpha" in the data and the cache.
fn with_alpha() -> Vec<u8> {
    let (r, out) = edit(&src(), |sa| sa.set_text(NODE, "alpha"));
    r.unwrap();
    out
}

#[test]
fn each_cache_invariant_refuses_on_its_own() {
    let base = with_alpha();
    // (1) an extra paragraph in the cache shape (same text, same runs)
    let extra = patch(&base, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        d.replace(&sp, &sp.replacen("</a:p>", "</a:p><a:p><a:endParaRPr lang=\"en-US\"/></a:p>", 1))
    });
    let (r, out) = edit(&extra, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(ref m)) if m.contains("paragraphs"), "{r:?}");
    assert_eq!(out, unchanged(&extra));
    // (1) the same runs, another text
    let other = patch(&base, DRAWING, |d| d.replace(">alpha<", ">omega<"));
    let (r, out) = edit(&other, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(ref m)) if m.contains("text"), "{r:?}");
    assert_eq!(out, unchanged(&other));
    // (3) the same text in two runs in the cache
    let split = patch(&base, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        let i = sp.find("<a:r>").unwrap();
        let j = sp.find("</a:r>").unwrap() + 6;
        let run = &sp[i..j];
        let two = run.replace(">alpha<", ">al<") + &run.replace(">alpha<", ">pha<");
        d.replace(&sp, &sp.replace(run, &two))
    });
    let (r, out) = edit(&split, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(ref m)) if m.contains("runs"), "{r:?}");
    assert_eq!(out, unchanged(&split));
}

#[test]
fn a_node_in_an_aggregate_shape_edits_its_own_slice() {
    // make the second node a second source of SHAPE (destOrd 1), shown as
    // a second paragraph there
    const SECOND: &str = "{4BB3DAE8-0D35-4C26-B00F-2F5FBE800636}";
    let base = with_alpha();
    let b = patch(&base, DATA, |d| {
        let i = d.find(&format!("type=\"presOf\" srcId=\"{SECOND}\"")).unwrap();
        let s = d[..i].rfind("<dgm:cxn ").unwrap();
        let e = i + d[i..].find("/>").unwrap() + 2;
        let cxn = &d[s..e];
        let dest = cxn.split("destId=\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
        d.replace(cxn, &cxn.replace(&dest, SHAPE).replace("destOrd=\"0\"", "destOrd=\"1\""))
    });
    let b = patch(&b, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        d.replace(&sp, &sp.replacen("</a:p>", "</a:p><a:p><a:endParaRPr lang=\"en-US\"/></a:p>", 1))
    });
    let (r, out) = edit(&b, |sa| sa.set_text(SECOND, "beta"));
    r.unwrap();
    assert_eq!(texts(&shape_xml(&part(&out, DRAWING), SHAPE)), ["alpha", "beta"]);
    let mut q = Presentation::from_bytes(out).unwrap();
    assert_eq!(q.smart_art_mut(0, 4).unwrap().cache_report().unwrap().consistent, 5);
}

#[test]
fn text_into_an_empty_data_run_goes_after_the_preceding_run() {
    // data: "A" then an empty run; cache: "A" only
    let (r, out) = edit(&src(), |sa| sa.set_text(NODE, "A"));
    r.unwrap();
    let b = patch(&out, DATA, |d| {
        let pt = point_xml(&d, NODE);
        d.replace(&pt, &pt.replacen("</a:r>", "</a:r><a:r><a:rPr lang=\"en-US\"/><a:t></a:t></a:r>", 1))
    });
    let (r, out) = edit(&b, |sa| sa.set_run_text(NODE, 0, 1, "B"));
    r.unwrap();
    assert_eq!(texts(&point_xml(&part(&out, DATA), NODE)), ["A", "B"]);
    let sp = shape_xml(&part(&out, DRAWING), SHAPE);
    assert_eq!(texts(&sp), ["A", "B"], "{sp}");
    // the new cache run copies the cache's own run format
    assert_eq!(sp.matches("<a:rPr lang=\"ru-RU\" sz=\"6500\" kern=\"1200\"/>").count(), 2, "{sp}");
}

// ------------------------------------------------------------------ review #63 round 1

/// `NODE`'s data paragraph is "A" followed by `extra` (raw runs); the cache
/// shows "A" (with `cache_rpr` as its run's a:rPr).
fn a_then(extra: &str, cache_rpr: Option<&str>) -> Vec<u8> {
    let (r, out) = edit(&src(), |sa| sa.set_text(NODE, "A"));
    r.unwrap();
    let b = patch(&out, DATA, |d| {
        let pt = point_xml(&d, NODE);
        d.replace(&pt, &pt.replacen("</a:r>", &format!("</a:r>{extra}"), 1))
    });
    match cache_rpr {
        None => b,
        Some(rpr) => patch(&b, DRAWING, |d| {
            let sp = shape_xml(&d, SHAPE);
            let i = sp.find("<a:rPr").unwrap();
            let j = i + sp[i..].find("/>").unwrap() + 2;
            d.replace(&sp, &format!("{}{rpr}{}", &sp[..i], &sp[j..]))
        }),
    }
}

const EMPTY_RUN: &str = "<a:r><a:t></a:t></a:r>";
const LINK_RPR: &str = "<a:rPr lang=\"en-US\"><a:hlinkClick xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"rId1\"/></a:rPr>";

#[test]
fn writing_into_an_empty_linked_data_run_is_refused() {
    // review M1: the data run written is checked, also when it is empty
    let b = a_then(&format!("{EMPTY_RUN}<a:r>{LINK_RPR}<a:t></a:t></a:r>"), None);
    // The linked run is a:r index 2, with only one preceding run with text.
    let (r, out) = edit(&b, |sa| sa.set_run_text(NODE, 0, 2, "LINK"));
    assert_matches!(r, Err(Error::FormatNotCopyable("a:rPr")), "{r:?}");
    assert_eq!(out, unchanged(&b));
}

#[test]
fn paragraph_text_refuses_links_in_either_part() {
    let linked_run = format!("<a:r>{LINK_RPR}<a:t></a:t></a:r>");
    let data_link = a_then(&linked_run, None);
    let cache_link = patch(&a_then("", None), DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        d.replace(&sp, &sp.replacen("</a:r>", &format!("</a:r>{linked_run}"), 1))
    });
    for b in [data_link, cache_link] {
        let (r, out) = edit(&b, |sa| sa.set_paragraph_text(NODE, 0, "B"));
        assert_matches!(r, Err(Error::FormatNotCopyable("a:rPr")), "{r:?}");
        assert_eq!(out, unchanged(&b));
    }
}

#[test]
fn run_text_refuses_a_link_on_the_paired_cache_run() {
    let cache_rpr = LINK_RPR.replace("lang=\"en-US\"", "lang=\"ru-RU\" sz=\"6500\" kern=\"1200\"");
    let b = a_then("", Some(&cache_rpr));
    let (r, out) = edit(&b, |sa| sa.set_run_text(NODE, 0, 0, "B"));
    assert_matches!(r, Err(Error::FormatNotCopyable("a:rPr")), "{r:?}");
    assert_eq!(out, unchanged(&b));
}

#[test]
fn copying_the_format_of_a_linked_cache_run_is_refused() {
    // review X1: a new cache run would copy a linked cache run's format
    let b = a_then("<a:r><a:rPr lang=\"en-US\"/><a:t></a:t></a:r>", Some(&LINK_RPR.replace("lang=\"en-US\"", "lang=\"ru-RU\" sz=\"6500\" kern=\"1200\"")));
    let (r, out) = edit(&b, |sa| sa.set_run_text(NODE, 0, 1, "B"));
    assert_matches!(r, Err(Error::FormatNotCopyable("a:rPr")), "{r:?}");
    assert_eq!(out, unchanged(&b));
}

#[test]
fn an_edit_that_would_unpair_the_cache_is_refused() {
    // review M2: the empty data run has its own format (b="1") that the
    // copied cache format lacks; the edit would leave the node inconsistent
    let b = a_then("<a:r><a:rPr lang=\"en-US\" b=\"1\"/><a:t></a:t></a:r>", None);
    let (r, out) = edit(&b, |sa| sa.set_run_text(NODE, 0, 1, "B"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(_)), "{r:?}");
    assert_eq!(out, unchanged(&b));
}

#[test]
fn a_node_shown_twice_in_one_shape_is_refused() {
    // review m1: a second presOf from NODE to SHAPE (destOrd 1)
    let base = {
        let (r, out) = edit(&src(), |sa| sa.set_text(NODE, "alpha"));
        r.unwrap();
        out
    };
    let b = patch(&base, DATA, |d| {
        d.replace("</dgm:cxnLst>", &format!("<dgm:cxn modelId=\"{{99999999-0000-0000-0000-000000000000}}\" type=\"presOf\" srcId=\"{NODE}\" destId=\"{SHAPE}\" srcOrd=\"0\" destOrd=\"1\"/></dgm:cxnLst>"))
    });
    let b = patch(&b, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        let i = sp.find("<a:p>").unwrap();
        let j = sp.find("</a:p>").unwrap() + 6;
        let para = sp[i..j].to_owned();
        d.replace(&sp, &sp.replacen(&para, &format!("{para}{para}"), 1))
    });
    let (r, out) = edit(&b, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheInconsistent(_)), "{r:?}");
    assert_eq!(out, unchanged(&b));
}

#[test]
fn relationships_of_the_wrong_type_are_not_followed() {
    // review m2: dataModelExt naming a relationship that is not diagramDrawing
    let b = patch(&src(), DATA, |d| d.replace("relId=\"rId6\"", "relId=\"rId1\"")); // rId1 = slideLayout
    let (r, out) = edit(&b, |sa| sa.set_text(NODE, "x"));
    assert_matches!(r, Err(Error::SmartArtCacheUnresolved));
    assert_eq!(out, unchanged(&b));
    // r:dm naming a relationship that is not diagramData
    let b = patch(&src(), "ppt/slides/slide1.xml", |s| s.replace("r:dm=\"rId2\"", "r:dm=\"rId3\""));
    let mut p = Presentation::from_bytes(b).unwrap();
    assert_matches!(p.smart_art_mut(0, 4), Err(Error::NotSmartArt));
}

#[test]
fn placeholders_keep_phldr_for_empty_text() {
    // review X5 / ADR m8: empty text does not touch phldr
    let (r, out) = edit(&src(), |sa| sa.set_text(NODE, ""));
    r.unwrap();
    assert!(point_xml(&part(&out, DATA), NODE).contains("phldr=\"1\""));
}

#[test]
fn text_into_an_empty_first_data_run_copies_the_first_cache_run() {
    // review X7: at the paragraph start the new cache run copies the first cache run's format
    let (r, out) = edit(&src(), |sa| sa.set_text(NODE, "A"));
    r.unwrap();
    let b = patch(&out, DATA, |d| {
        let pt = point_xml(&d, NODE);
        d.replace(&pt, &pt.replacen("<a:r>", "<a:r><a:rPr lang=\"en-US\"/><a:t></a:t></a:r><a:r>", 1))
    });
    let (r, out) = edit(&b, |sa| sa.set_run_text(NODE, 0, 0, "B"));
    r.unwrap();
    let sp = shape_xml(&part(&out, DRAWING), SHAPE);
    assert_eq!(texts(&sp), ["B", "A"]);
    assert_eq!(sp.matches("<a:rPr lang=\"ru-RU\" sz=\"6500\" kern=\"1200\"/>").count(), 2, "{sp}");
}

#[test]
fn a_middle_node_of_an_aggregate_keeps_its_place() {
    // review X6: three sources in SHAPE; the middle one gets two lines
    const SECOND: &str = "{4BB3DAE8-0D35-4C26-B00F-2F5FBE800636}";
    const THIRD: &str = "{45312779-31C8-4B7E-86E5-0623F1579DAF}";
    let base = with_alpha();
    let redirect = |d: String, id: &str, ord: u32| {
        let i = d.find(&format!("type=\"presOf\" srcId=\"{id}\"")).unwrap();
        let s = d[..i].rfind("<dgm:cxn ").unwrap();
        let e = i + d[i..].find("/>").unwrap() + 2;
        let cxn = d[s..e].to_owned();
        let dest = cxn.split("destId=\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
        d.replace(&cxn, &cxn.replace(&dest, SHAPE).replace("destOrd=\"0\"", &format!("destOrd=\"{ord}\"")))
    };
    let b = patch(&base, DATA, |d| redirect(redirect(d, SECOND, 1), THIRD, 2));
    let b = patch(&b, DRAWING, |d| {
        let sp = shape_xml(&d, SHAPE);
        d.replace(&sp, &sp.replacen("</a:p>", "</a:p><a:p><a:endParaRPr lang=\"en-US\"/></a:p><a:p><a:endParaRPr lang=\"en-US\"/></a:p>", 1))
    });
    let (r, out) = edit(&b, |sa| sa.set_text(THIRD, "gamma"));
    r.unwrap();
    let (r, out) = edit(&out, |sa| sa.set_text(SECOND, "beta one\nbeta two"));
    r.unwrap();
    assert_eq!(texts(&shape_xml(&part(&out, DRAWING), SHAPE)), ["alpha", "beta one", "beta two", "gamma"]);
}

#[test]
fn drop_keeps_other_extensions_and_removes_drawing_only_parts() {
    // review X2, X3: an image only the drawing uses goes with it; another
    // dgm:extLst extension stays
    let b = patch(&src(), DATA, |d| {
        d.replace("</dgm:extLst>", "<a:ext uri=\"{OTHER}\"><x:keep xmlns:x=\"urn:x\"/></a:ext></dgm:extLst>")
    });
    let b = patch(&b, "ppt/diagrams/_rels/drawing1.xml.rels", |_| {
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/only.jpeg\"/></Relationships>".into()
    });
    let b = patch(&b, "ppt/media/only.jpeg", |_| "jpeg".into());
    let (r, out) = edit(&b, |sa| sa.drop_drawing_cache());
    r.unwrap();
    assert!(!has(&out, "ppt/media/only.jpeg"), "a part only the drawing referenced goes with it");
    let d = part(&out, DATA);
    assert!(d.contains("<dgm:extLst><a:ext uri=\"{OTHER}\">") && !d.contains("dataModelExt"), "{}", &d[d.find("<dgm:extLst").unwrap()..]);
}
