//! Phase 2 (ADR-0012 §10): slides as units. L1 locality (untouched parts
//! keep their compressed bytes; registries only gain or lose entries),
//! per-operation results, T4 refusals (byte-identical output), session
//! allocation, sections, `app.xml`, and a sweep over every fixture slide
//! with structural invariants checked on the reopened output.

use std::assert_matches;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use ooxml_pptx::{Error, LayoutRef, Presentation};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx");

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{rel}")).unwrap()
}

/// `name -> (crc32, compressed bytes)`.
fn raw_entries(b: &[u8]) -> BTreeMap<String, (u32, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index_raw(i).unwrap();
        let mut v = Vec::new();
        f.read_to_end(&mut v).unwrap();
        out.insert(f.name().unwrap().into_owned(), (f.crc32(), v));
    }
    out
}

fn part(b: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

fn has(b: &[u8], name: &str) -> bool {
    zip::ZipArchive::new(Cursor::new(b))
        .unwrap()
        .by_name(name)
        .is_ok()
}

/// Rewrites one entry (or adds it) in a package.
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

/// The output of an untouched open → save, the reference for "nothing
/// changed".
fn unchanged(b: &[u8]) -> Vec<u8> {
    Presentation::from_bytes(b.to_vec())
        .unwrap()
        .to_bytes()
        .unwrap()
}

fn ids(p: &Presentation) -> Vec<u32> {
    p.slides().map(|s| s.unwrap().slide_id).collect()
}

fn layout(p: &mut Presentation, name: &str) -> LayoutRef {
    p.layouts()
        .unwrap()
        .into_iter()
        .find(|l| l.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("layout {name}"))
}

/// Every part except `changed` keeps its compressed bytes; `changed`
/// registries keep every original line (entries only appended or removed).
fn assert_local(before: &[u8], after: &[u8], changed: &[&str]) {
    let a = raw_entries(before);
    let b = raw_entries(after);
    for (name, v) in &a {
        if changed.contains(&name.as_str()) {
            continue;
        }
        if let Some(w) = b.get(name) {
            assert_eq!(v, w, "{name} changed");
        }
    }
}

/// The text outside `<tag …>…</tag>` is unchanged.
fn same_outside(before: &str, after: &str, tag: &str) {
    let cut = |s: &str| {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let i = s.find(&open).unwrap_or(s.len());
        let j = s.find(&close).map_or(i, |j| j + close.len());
        (s[..i].to_owned(), s[j..].to_owned())
    };
    assert_eq!(cut(before), cut(after), "outside <{tag}> changed");
}

/// Structural invariants of a package (ADR-0012 §10, structural oracle).
fn check_structure(b: &[u8]) {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let names: Vec<String> = (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().unwrap().into_owned())
        .collect();
    let lower: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let mut ct = String::new();
    z.by_name("[Content_Types].xml")
        .unwrap()
        .read_to_string(&mut ct)
        .unwrap();
    for n in &names {
        if n.ends_with('/') || n == "[Content_Types].xml" {
            continue;
        }
        let ov = ct
            .to_ascii_lowercase()
            .contains(&format!("partname=\"/{}\"", n.to_ascii_lowercase()));
        let ext = n.rsplit('.').next().unwrap().to_ascii_lowercase();
        let def = ct
            .to_ascii_lowercase()
            .contains(&format!("extension=\"{ext}\""));
        assert!(ov || def, "{n} has no content type");
    }
    // every internal relationship target of a new-style rels part exists
    for (i, n) in names.iter().enumerate() {
        if !lower[i].ends_with(".rels") {
            continue;
        }
        let mut s = String::new();
        z.by_name(n).unwrap().read_to_string(&mut s).unwrap();
        let dir = n
            .rsplit_once("_rels/")
            .map(|(d, _)| d.to_owned())
            .unwrap_or_default();
        for rel in s.split("<Relationship ").skip(1) {
            if rel.contains("TargetMode=\"External\"") {
                continue;
            }
            let t = rel
                .split("Target=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            if t.contains('#') || t.contains(':') {
                continue;
            }
            let full = if let Some(abs) = t.strip_prefix('/') {
                abs.to_owned()
            } else {
                normalize(&format!("{dir}{t}"))
            };
            assert!(
                lower.contains(&full.to_ascii_lowercase()) || full.ends_with('/'),
                "{n}: dangling target {t} ({full})"
            );
        }
    }
    // sldIds unique, in range; sections concatenate to the list
    let p = Presentation::from_bytes(b.to_vec()).unwrap();
    let v = ids(&p);
    let mut u = v.clone();
    u.sort_unstable();
    u.dedup();
    assert_eq!(u.len(), v.len(), "duplicate sldId");
    assert!(v.iter().all(|&i| (256..=2_147_483_647).contains(&i)));
    let pres = part(b, "ppt/presentation.xml");
    if let Some(sec) = pres.split("<p14:sectionLst").nth(1) {
        let sec = sec.split("</p14:sectionLst>").next().unwrap();
        let sids: Vec<u32> = sec
            .split("<p14:sldId ")
            .skip(1)
            .map(|s| {
                s.split("id=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(sids, v, "sections do not concatenate to sldIdLst");
    }
}

fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            ".." => {
                out.pop();
            }
            "." | "" => {}
            s => out.push(s),
        }
    }
    out.join("/")
}

// ------------------------------------------------------------------ add

#[test]
fn add_slide_from_layout() {
    let src = fixture("poi/sample.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let before_ids = ids(&p);
    let l = layout(&mut p, "Title and Content");
    {
        let s = p.add_slide(&l, Some(1)).unwrap();
        assert_eq!(s.as_slide().info().index, 1);
    }
    let after_ids = ids(&p);
    let new = *before_ids.iter().max().unwrap() + 1;
    assert_eq!(after_ids, [before_ids[0], new, before_ids[1]]);
    let out = p.to_bytes().unwrap();
    let base = unchanged(&src);
    assert_local(
        &base,
        &out,
        &[
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "[Content_Types].xml",
            "docProps/app.xml",
        ],
    );
    same_outside(
        &part(&base, "ppt/presentation.xml"),
        &part(&out, "ppt/presentation.xml"),
        "p:sldIdLst",
    );
    // the new part: one more than the largest slide number, an Override,
    // one layout relationship, python-pptx's placeholder shape
    assert!(has(&out, "ppt/slides/slide3.xml"));
    assert!(part(&out, "[Content_Types].xml").contains("/ppt/slides/slide3.xml"));
    let rels = part(&out, "ppt/slides/_rels/slide3.xml.rels");
    assert_eq!(rels.matches("<Relationship ").count(), 1);
    assert!(
        rels.contains("relationships/slideLayout")
            && rels.contains(l.part.rsplit('/').next().unwrap())
    );
    let xml = part(&out, "ppt/slides/slide3.xml");
    assert!(xml.contains("<p:ph type=\"title\"/>"), "{xml}");
    assert!(xml.contains("name=\"Title 1\""));
    assert!(
        xml.contains("<p:ph idx=\"1\"/>") && xml.contains("name=\"Content Placeholder 2\""),
        "{xml}"
    );
    assert!(
        !xml.contains("type=\"dt\"")
            && !xml.contains("type=\"sldNum\"")
            && !xml.contains("type=\"ftr\"")
    );
    assert!(!xml.contains("<a:t>"), "no prompt text");
    // registries: the original lines are all still there, in order
    let ct0 = part(&base, "[Content_Types].xml");
    let ct1 = part(&out, "[Content_Types].xml");
    assert!(
        ct1.starts_with(ct0.trim_end_matches("</Types>")),
        "content types reordered"
    );
    let r0 = part(&base, "ppt/_rels/presentation.xml.rels");
    let r1 = part(&out, "ppt/_rels/presentation.xml.rels");
    assert!(r1.starts_with(r0.trim_end_matches("</Relationships>")));
    check_structure(&out);
    // the placeholder is editable through the Phase 0 API
    let mut p = Presentation::from_bytes(out).unwrap();
    let mut s = p.slide_mut(1).unwrap();
    let path = s.as_slide().placeholders()[0].path().clone();
    s.shape_mut(&path)
        .unwrap()
        .text_body_mut()
        .unwrap()
        .set_text("Hello")
        .unwrap();
    let out = p.to_bytes().unwrap();
    assert!(part(&out, "ppt/slides/slide3.xml").contains("<a:t>Hello</a:t>"));
}

#[test]
fn add_to_an_empty_list_and_at_the_end() {
    // a deck whose only slide was deleted has no p:sldId left; add creates one
    let src = fixture("python-pptx/sld-notes.pptx");
    let mut p = Presentation::from_bytes(src).unwrap();
    p.delete_slide(1).unwrap();
    p.delete_slide(0).unwrap();
    assert_eq!(p.slide_count(), 0);
    let l = p.layouts().unwrap()[0].clone();
    p.add_slide(&l, None).unwrap();
    p.add_slide(&l, None).unwrap();
    assert_eq!(p.slide_count(), 2);
    let out = p.to_bytes().unwrap();
    check_structure(&out);
}

#[test]
fn slide_ids_use_a_session_high_water_mark() {
    let src = fixture("poi/sample.pptx");
    let mut p = Presentation::from_bytes(src).unwrap();
    let max = *ids(&p).iter().max().unwrap();
    let l = p.layouts().unwrap()[0].clone();
    p.add_slide(&l, None).unwrap();
    assert_eq!(*ids(&p).last().unwrap(), max + 1);
    p.delete_slide(2).unwrap();
    p.add_slide(&l, None).unwrap();
    assert_eq!(
        *ids(&p).last().unwrap(),
        max + 2,
        "an id removed in this session is not reused"
    );
    // part numbers and rIds are never reused either (core rule)
    let out = p.to_bytes().unwrap();
    assert!(!has(&out, "ppt/slides/slide3.xml") && has(&out, "ppt/slides/slide4.xml"));
}

#[test]
fn slide_ids_exhausted() {
    let src = patch(&fixture("poi/sample.pptx"), "ppt/presentation.xml", |s| {
        let (a, b) = s.split_once("<p:sldId id=\"").unwrap();
        let (_, rest) = b.split_once('"').unwrap();
        format!("{a}<p:sldId id=\"2147483647\"{rest}")
    });
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let l = p.layouts().unwrap()[0].clone();
    assert_matches!(p.add_slide(&l, None), Err(Error::SlideIdsExhausted));
    assert_matches!(p.duplicate_slide(0, None), Err(Error::SlideIdsExhausted));
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
}

#[test]
fn layout_not_found_and_bad_positions() {
    let src = fixture("poi/sample.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let mut l = p.layouts().unwrap()[0].clone();
    assert_matches!(
        p.add_slide(&l, Some(3)),
        Err(Error::SlideIndexOutOfRange { index: 3, count: 2 })
    );
    l.part = "ppt/slideLayouts/nope.xml".into();
    assert_matches!(p.add_slide(&l, None), Err(Error::LayoutNotFound(_)));
    assert_matches!(p.move_slide(0, 2), Err(Error::SlideIndexOutOfRange { .. }));
    assert_matches!(p.delete_slide(2), Err(Error::SlideIndexOutOfRange { .. }));
    assert_matches!(
        p.duplicate_slide(0, Some(3)),
        Err(Error::SlideIndexOutOfRange { .. })
    );
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
}

// ------------------------------------------------------------------ move

#[test]
fn move_changes_only_the_list() {
    let src = fixture("poi/layouts.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let mut v = ids(&p);
    p.move_slide(0, 4).unwrap();
    let x = v.remove(0);
    v.insert(4, x);
    assert_eq!(ids(&p), v);
    p.move_slide(4, 4).unwrap();
    let out = p.to_bytes().unwrap();
    let base = unchanged(&src);
    assert_local(&base, &out, &["ppt/presentation.xml"]);
    same_outside(
        &part(&base, "ppt/presentation.xml"),
        &part(&out, "ppt/presentation.xml"),
        "p:sldIdLst",
    );
    check_structure(&out);
}

// ------------------------------------------------------------------ sections

fn section_lists(b: &[u8]) -> Vec<Vec<u32>> {
    let pres = part(b, "ppt/presentation.xml");
    let sec = pres
        .split("<p14:sectionLst")
        .nth(1)
        .unwrap()
        .split("</p14:sectionLst>")
        .next()
        .unwrap();
    sec.split("<p14:section ")
        .skip(1)
        .map(|s| {
            s.split("<p14:sldId ")
                .skip(1)
                .map(|x| {
                    x.split("id=\"")
                        .nth(1)
                        .unwrap()
                        .split('"')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap()
                })
                .collect()
        })
        .collect()
}

#[test]
fn sections_follow_add_move_delete_duplicate() {
    let src = fixture("libreoffice/slide-sections.pptx");
    let s0 = section_lists(&src);
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let v = ids(&p);
    let l = p.layouts().unwrap()[0].clone();
    // add after the first slide: joins its section, right after it
    p.add_slide(&l, Some(1)).unwrap();
    let new = *ids(&p).iter().max().unwrap();
    let s1 = section_lists(&p.to_bytes().unwrap());
    let k = s0.iter().position(|s| s.contains(&v[0])).unwrap();
    let at = s0[k].iter().position(|&x| x == v[0]).unwrap() + 1;
    let mut expect = s0.clone();
    expect[k].insert(at, new);
    assert_eq!(s1, expect);
    // add at position 0: the section of the slide now first
    p.add_slide(&l, Some(0)).unwrap();
    let new0 = *ids(&p).iter().max().unwrap();
    let s2 = section_lists(&p.to_bytes().unwrap());
    let k0 = expect
        .iter()
        .position(|s| s.first() == Some(&v[0]) || s.contains(&v[0]))
        .unwrap();
    expect[k0].insert(0, new0);
    assert_eq!(s2, expect);
    // moving the last slide first, deleting, duplicating keep the invariant
    let n = p.slide_count();
    p.move_slide(n - 1, 0).unwrap();
    p.delete_slide(2).unwrap();
    p.duplicate_slide(1, None).unwrap();
    let out = p.to_bytes().unwrap();
    check_structure(&out);
    // a section emptied by deletes stays
    let mut p = Presentation::from_bytes(out).unwrap();
    while p.slide_count() > 1 {
        p.delete_slide(0).unwrap();
    }
    let out = p.to_bytes().unwrap();
    assert_eq!(section_lists(&out).len(), s0.len());
    check_structure(&out);
}

#[test]
fn inconsistent_or_unsupported_sections_refuse() {
    let src = patch(
        &fixture("libreoffice/slide-sections.pptx"),
        "ppt/presentation.xml",
        |s| {
            // drop one p14:sldId: the sections no longer cover the list
            let i = s.find("<p14:sldId ").unwrap();
            let j = i + s[i..].find("/>").unwrap() + 2;
            format!("{}{}", &s[..i], &s[j..])
        },
    );
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.move_slide(0, 1), Err(Error::SectionsInconsistent));
    assert_matches!(p.delete_slide(0), Err(Error::SectionsInconsistent));
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
    let src = patch(
        &fixture("libreoffice/slide-sections.pptx"),
        "ppt/presentation.xml",
        |s| {
            s.replace(
                "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}",
                "{00000000-0000-0000-0000-000000000000}",
            )
        },
    );
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.move_slide(0, 1), Err(Error::UnsupportedSections));
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
}

// ------------------------------------------------------------------ delete

#[test]
fn delete_cascades_notes_and_unshared_media() {
    let src = fixture("python-pptx/sld-notes.pptx");
    let base = unchanged(&src);
    let rels0 = part(&src, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels0.contains("notesSlide"));
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let id1 = ids(&p)[1];
    p.delete_slide(0).unwrap();
    assert_eq!(ids(&p), [id1]);
    let out = p.to_bytes().unwrap();
    assert!(!has(&out, "ppt/slides/slide1.xml") && !has(&out, "ppt/slides/_rels/slide1.xml.rels"));
    let notes = rels0
        .split("notesSlides/")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(
        !has(&out, &format!("ppt/notesSlides/{notes}")),
        "notes slide removed with its slide"
    );
    assert!(
        has(&out, "ppt/notesMasters/notesMaster1.xml"),
        "notes master kept"
    );
    assert!(!part(&out, "[Content_Types].xml").contains("/ppt/slides/slide1.xml"));
    assert_local(
        &base,
        &out,
        &[
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "[Content_Types].xml",
            "docProps/app.xml",
            "ppt/viewProps.xml",
        ],
    );
    check_structure(&out);
}

#[test]
fn delete_keeps_shared_media() {
    // layouts.pptx: media/image1.jpg is used by one slide only
    let src = fixture("poi/layouts.pptx");
    let p0 = Presentation::from_bytes(src.clone()).unwrap();
    let parts: Vec<String> = p0.slides().map(|s| s.unwrap().part_name).collect();
    let i = parts
        .iter()
        .position(|n| {
            let (d, f) = n.rsplit_once('/').unwrap();
            has(&src, &format!("{d}/_rels/{f}.rels"))
                && part(&src, &format!("{d}/_rels/{f}.rels")).contains("media/image1.jpg")
        })
        .unwrap();
    let media = "ppt/media/image1.jpg";
    let mut p = Presentation::from_bytes(src).unwrap();
    // the copy shares the image, so deleting the original keeps it
    p.duplicate_slide(i, None).unwrap();
    p.delete_slide(i).unwrap();
    let out = p.to_bytes().unwrap();
    assert!(has(&out, media), "media shared with the copy was removed");
    check_structure(&out);
    // deleting its last user cascades it
    p.delete_slide(i).unwrap();
    let out = p.to_bytes().unwrap();
    assert!(!has(&out, media), "media of the last user cascades");
    check_structure(&out);
}

#[test]
fn app_xml_counts() {
    let count = |b: &[u8], tag: &str| -> i64 {
        let a = part(b, "docProps/app.xml");
        a.split(&format!("<{tag}>"))
            .nth(1)
            .unwrap()
            .split('<')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let src = fixture("python-pptx/sld-notes.pptx");
    let (s0, n0) = (count(&src, "Slides"), count(&src, "Notes"));
    let mut p = Presentation::from_bytes(src).unwrap();
    p.duplicate_slide(0, None).unwrap();
    let out = p.to_bytes().unwrap();
    assert_eq!(
        (count(&out, "Slides"), count(&out, "Notes")),
        (s0 + 1, n0 + 1)
    );
    p.delete_slide(0).unwrap();
    p.delete_slide(0).unwrap();
    let out = p.to_bytes().unwrap();
    assert_eq!(
        (count(&out, "Slides"), count(&out, "Notes")),
        (s0 - 1, n0 - 1)
    );
    let l = p.layouts().unwrap()[0].clone();
    p.add_slide(&l, None).unwrap();
    let out = p.to_bytes().unwrap();
    assert_eq!((count(&out, "Slides"), count(&out, "Notes")), (s0, n0 - 1));
    // titles lists are left as they are
    let a = part(&out, "docProps/app.xml");
    assert_eq!(
        a.split("<TitlesOfParts>").nth(1),
        part(&fixture("python-pptx/sld-notes.pptx"), "docProps/app.xml")
            .split("<TitlesOfParts>")
            .nth(1)
    );
}

#[test]
fn delete_refusals_leave_the_package_unchanged() {
    // a jump link from another slide
    let src = fixture("poi/sample.pptx");
    let src = patch(&src, "ppt/slides/_rels/slide2.xml.rels", |s| {
        s.replace("</Relationships>", "<Relationship Id=\"rId99\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slide1.xml\"/></Relationships>")
    });
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.delete_slide(0), Err(Error::SlideIsReferenced { ref from_part, ref rel_type }) if from_part == "ppt/slides/slide2.xml" && rel_type.ends_with("/slide"));
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
    // a Slide Zoom naming the slide's id
    let src0 = fixture("poi/sample.pptx");
    let id = ids(&Presentation::from_bytes(src0.clone()).unwrap())[0];
    let src = patch(&src0, "ppt/slides/slide2.xml", |s| {
        s.replace("</p:sld>", &format!("<p:extLst><p:ext uri=\"{{x}}\"><pslz:sldZmObj xmlns:pslz=\"http://schemas.microsoft.com/office/powerpoint/2016/slidezoom\" sldId=\"{id}\"/></p:ext></p:extLst></p:sld>"))
    });
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.delete_slide(0), Err(Error::SlideIsReferenced { ref rel_type, .. }) if rel_type == "sldId");
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
    // a custom show
    let src = patch(&src0, "ppt/presentation.xml", |s| {
        let rid = s
            .split("<p:sldId ")
            .nth(1)
            .unwrap()
            .split("r:id=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned();
        s.replace("<p:sldSz", &format!("<p:custShowLst><p:custShow name=\"x\" id=\"0\"><p:sldLst><p:sld r:id=\"{rid}\"/></p:sldLst></p:custShow></p:custShowLst><p:sldSz"))
    });
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.delete_slide(0), Err(Error::SlideInCustomShow));
    p.delete_slide(1).unwrap(); // the other slide is not in the show
    // a positional show range refuses every position change
    let src = patch(&src0, "ppt/presProps.xml", |_| {
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:presentationPr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:showPr><p:sldRg st=\"1\" end=\"2\"/></p:showPr></p:presentationPr>".into()
    });
    {
        let mut p = Presentation::from_bytes(src.clone()).unwrap();
        let l = p.layouts().unwrap()[0].clone();
        assert_matches!(p.delete_slide(0), Err(Error::ShowRangeByPosition));
        assert_matches!(p.move_slide(0, 1), Err(Error::ShowRangeByPosition));
        assert_matches!(p.duplicate_slide(0, None), Err(Error::ShowRangeByPosition));
        assert_matches!(p.add_slide(&l, None), Err(Error::ShowRangeByPosition));
        assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
    }
    // an unreadable relationships part anywhere fails closed
    let src = patch(
        &src0,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        |_| "<not-rels".into(),
    );
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.delete_slide(0), Err(Error::SlideIsReferenced { ref rel_type, .. }) if rel_type == "?");
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
    // an unsupported cascade type
    let src = fixture("open-xml-sdk/Of16-01.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.delete_slide(0), Err(Error::UnsupportedSlidePart(t)) if t.ends_with("chartEx"));
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
}

#[test]
fn delete_removes_viewprops_outline_entries() {
    let src0 = fixture("poi/sample.pptx");
    // give viewProps an outline entry for slide 1 (relationship + p:sld)
    let src = patch(&src0, "ppt/_rels/viewProps.xml.rels", |_| {
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide1.xml\"/></Relationships>".into()
    });
    let src = patch(&src, "ppt/viewProps.xml", |s| {
        let i = s.find("</p:viewPr>").unwrap();
        if s.contains("<p:outlineViewPr") {
            s.clone()
        } else {
            let anchor = s
                .find("<p:notesTextViewPr")
                .or_else(|| s.find("<p:gridSpacing"))
                .unwrap_or(i);
            format!(
                "{}<p:outlineViewPr><p:cViewPr><p:scale><a:sx n=\"33\" d=\"100\"/><a:sy n=\"33\" d=\"100\"/></p:scale><p:origin x=\"0\" y=\"0\"/></p:cViewPr><p:sldLst><p:sld r:id=\"rId1\" collapse=\"1\"/></p:sldLst></p:outlineViewPr>{}",
                &s[..anchor],
                &s[anchor..]
            )
        }
    });
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    p.delete_slide(0).unwrap();
    let out = p.to_bytes().unwrap();
    let vp = part(&out, "ppt/viewProps.xml");
    assert!(
        vp.contains("<p:sldLst></p:sldLst>"),
        "outline entry removed, list kept: {vp}"
    );
    assert!(!part(&out, "ppt/_rels/viewProps.xml.rels").contains("slides/slide1.xml"));
    check_structure(&out);
}

// ------------------------------------------------------------------ duplicate

#[test]
fn duplicate_copies_notes_and_keeps_ids() {
    let src = fixture("python-pptx/sld-notes.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let v = ids(&p);
    p.duplicate_slide(0, None).unwrap();
    let w = ids(&p);
    assert_eq!(w.len(), 3);
    assert_eq!((w[0], w[2]), (v[0], v[1]));
    let out = p.to_bytes().unwrap();
    let new_slide = "ppt/slides/slide3.xml";
    let strip = |s: String| s.split("<p14:creationId").next().unwrap().to_owned();
    assert_eq!(
        strip(part(&out, new_slide)),
        strip(part(&src, "ppt/slides/slide1.xml")),
        "the copy's XML is the original's up to its creationId"
    );
    let r0 = part(&src, "ppt/slides/_rels/slide1.xml.rels");
    let r1 = part(&out, "ppt/slides/_rels/slide3.xml.rels");
    let rid_set = |s: &str| -> Vec<String> {
        s.split("Id=\"")
            .skip(1)
            .map(|x| x.split('"').next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(rid_set(&r0), rid_set(&r1), "same relationship Ids");
    let notes_copy = r1
        .split("notesSlides/")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(
        r0.split("notesSlides/")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            != notes_copy
    );
    let nr = part(&out, &format!("ppt/notesSlides/_rels/{notes_copy}.rels"));
    assert!(
        nr.contains("slides/slide3.xml"),
        "notes back-relationship maps to the copy: {nr}"
    );
    assert!(nr.contains("notesMaster"), "notes master shared");
    check_structure(&out);
}

#[test]
fn duplicate_renews_creation_id() {
    // sld-notes.pptx: slides and notes slides carry p:cSld/p:extLst/…/p14:creationId
    let src = fixture("python-pptx/sld-notes.pptx");
    let cid = |s: &str| {
        s.split("<p14:creationId").nth(1).map(|x| {
            x.split("val=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        })
    };
    let names: Vec<String> = raw_entries(&src)
        .into_keys()
        .filter(|n| n.ends_with(".xml") && (n.contains("/slides/") || n.contains("/notesSlides/")))
        .collect();
    let max = names
        .iter()
        .filter_map(|n| cid(&part(&src, n)))
        .max()
        .unwrap();
    let orig = cid(&part(&src, "ppt/slides/slide1.xml")).unwrap();
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let new_part = p
        .duplicate_slide(0, None)
        .unwrap()
        .as_slide()
        .info()
        .part_name
        .clone();
    let out = p.to_bytes().unwrap();
    let c = cid(&part(&out, &new_part)).unwrap();
    assert_eq!(c, max + 1);
    assert_ne!(c, orig);
    // the rest of the copy is the original's bytes
    assert_eq!(
        part(&out, &new_part).replace(&format!("val=\"{c}\""), &format!("val=\"{orig}\"")),
        part(&src, "ppt/slides/slide1.xml")
    );
    // the notes copy gets the next value
    let r = part(&out, "ppt/slides/_rels/slide3.xml.rels");
    let notes = r
        .split("notesSlides/")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_eq!(
        cid(&part(&out, &format!("ppt/notesSlides/{notes}"))),
        Some(max + 2)
    );
    check_structure(&out);
}

#[test]
fn duplicate_refusals() {
    for (f, want) in [
        ("libreoffice/modern-comments.pptx", "=comments"),
        ("poi/bar-chart.pptx", "relationships/chart"),
        ("poi/SmartArt.pptx", "relationships/diagram"),
    ] {
        let src = fixture(f);
        let mut p = Presentation::from_bytes(src.clone()).unwrap();
let r = p.duplicate_slide(0, None).map(|_| ());
        match want.strip_prefix('=') {
            Some(exact) => assert_matches!(r, Err(Error::UnsupportedSlidePart(ref t)) if t == exact, "{f}"),
            None => assert_matches!(r, Err(Error::UnsupportedSlidePart(ref t)) if t.contains(want), "{f}"),
        }
        assert_eq!(p.to_bytes().unwrap(), unchanged(&src), "{f}");
    }
    // a core refusal after the pptx edits are planned: `#_ftn1` targets
    // resolve to no part (copy_relationships refuses until #38 copies them)
    let src = fixture("poi/with_japanese.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.duplicate_slide(0, None), Err(Error::Core(_)));
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
    // a tags part with its own relationships
    let src = patch(
        &fixture("poi/sample.pptx"),
        "ppt/slides/_rels/slide1.xml.rels",
        |s| {
            s.replace("</Relationships>", "<Relationship Id=\"rId98\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/tags\" Target=\"../tags/tag1.xml\"/></Relationships>")
        },
    );
    let src = patch(&src, "ppt/tags/tag1.xml", |_| {
        "<p:tagLst xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"/>".into()
    });
    let src = patch(&src, "[Content_Types].xml", |s| {
        s.replace("</Types>", "<Override PartName=\"/ppt/tags/tag1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.tags+xml\"/></Types>")
    });
    // without own relationships the tags part is copied
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    p.duplicate_slide(0, None).unwrap();
    let out = p.to_bytes().unwrap();
    assert!(has(&out, "ppt/tags/tag2.xml"), "tags copied");
    assert!(part(&out, "ppt/slides/_rels/slide3.xml.rels").contains("../tags/tag2.xml"));
    check_structure(&out);
    let src = patch(&src, "ppt/tags/_rels/tag1.xml.rels", |_| {
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"http://x/\" TargetMode=\"External\"/></Relationships>".into()
    });
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    assert_matches!(p.duplicate_slide(0, None), Err(Error::UnsupportedSlidePart(ref t)) if t == "tags");
    assert_eq!(p.to_bytes().unwrap(), unchanged(&src));
}

// ------------------------------------------------------------------ shape ids

#[test]
fn shape_ids_cover_spid_and_comment_monikers() {
    let src = patch(&fixture("poi/sample.pptx"), "ppt/slides/slide1.xml", |s| {
        s.replace("</p:sld>", "<p:timing><p:tnLst><p:par><p:cTn id=\"1\"><p:childTnLst><p:set><p:cBhvr><p:tgtEl><p:spTgt spid=\"_x0000_s4097\"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>")
    });
    let mut p = Presentation::from_bytes(src).unwrap();
    assert_eq!(p.next_shape_id(0).unwrap(), 4098);
    let src = fixture("libreoffice/modern-comments.pptx");
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let slide_max = {
        let s = part(&src, &p.slides().next().unwrap().unwrap().part_name);
        s.split(" id=\"")
            .skip(1)
            .filter_map(|x| x.split('"').next().unwrap().parse::<u32>().ok())
            .max()
            .unwrap()
    };
    assert!(p.next_shape_id(0).unwrap() > slide_max);
}

// ------------------------------------------------------------------ sweep

/// Every slide of every fixture: delete it, and duplicate it, then reopen
/// and check the structure; a refusal leaves the package unchanged.
#[test]
fn sweep_every_fixture_slide() {
    let mut files = Vec::new();
    for d in ["libreoffice", "open-xml-sdk", "poi", "python-pptx"] {
        for e in std::fs::read_dir(format!("{FIX}/{d}")).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "pptx") {
                files.push(p);
            }
        }
    }
    files.sort();
    let (mut ok, mut refused) = (0, 0);
    for f in &files {
        let src = std::fs::read(f).unwrap();
        let base = unchanged(&src);
        let n = Presentation::from_bytes(src.clone()).unwrap().slide_count();
        for i in 0..n {
            for op in ["delete", "duplicate"] {
                let mut p = Presentation::from_bytes(src.clone()).unwrap();
                let r = match op {
                    "delete" => p.delete_slide(i),
                    _ => p.duplicate_slide(i, None).map(|_| ()),
                };
                let out = p.to_bytes().unwrap();
                match r {
                    Ok(()) => {
                        ok += 1;
                        check_structure(&out);
                        let q = Presentation::from_bytes(out).unwrap();
                        assert_eq!(q.slide_count(), if op == "delete" { n - 1 } else { n + 1 });
                    }
                    Err(e) => {
                        refused += 1;
                        assert_eq!(
                            out,
                            base,
                            "{} {op} {i}: refusal {e} changed the package",
                            f.display()
                        );
                    }
                }
            }
        }
    }
    assert!(ok > 80, "ok {ok}, refused {refused}");
}

#[test]
fn delete_cascades_through_a_chart_to_its_embedding() {
    let src = fixture("poi/bar-chart.pptx");
    let mut p = Presentation::from_bytes(src).unwrap();
    p.delete_slide(0).unwrap();
    let out = p.to_bytes().unwrap();
    for gone in ["ppt/charts/chart1.xml", "ppt/charts/_rels/chart1.xml.rels", "ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx"] {
        assert!(!has(&out, gone), "{gone} left behind");
    }
    check_structure(&out);
}

#[test]
fn picture_placeholders_get_no_text_body() {
    let src = fixture("poi/layouts.pptx");
    let mut p = Presentation::from_bytes(src).unwrap();
    let l = p.layouts().unwrap().into_iter().find(|l| l.part == "ppt/slideLayouts/slideLayout9.xml").unwrap();
    let part_name = p.add_slide(&l, None).unwrap().as_slide().info().part_name.clone();
    let out = p.to_bytes().unwrap();
    let xml = part(&out, &part_name);
    let pic = xml.split("<p:sp>").find(|sp| sp.contains("type=\"pic\"")).expect("a picture placeholder");
    assert!(!pic.contains("<p:txBody>"), "{pic}");
    assert!(pic.contains("name=\"Picture Placeholder"));
    let title = xml.split("<p:sp>").find(|sp| sp.contains("type=\"title\"")).unwrap();
    assert!(title.contains("<p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody>"));
}

#[test]
fn shape_ids_stay_above_comment_monikers() {
    let src = patch(&fixture("libreoffice/modern-comments.pptx"), "ppt/comments/modernComment_100_68C49C6.xml", |s| {
        s.replacen("<ac:spMk id=\"2\"", "<ac:spMk id=\"5000\"", 1)
    });
    let mut p = Presentation::from_bytes(src).unwrap();
    assert_eq!(p.next_shape_id(0).unwrap(), 5001);
}

#[test]
fn hidden_slides_are_counted() {
    let count = |b: &[u8]| -> i64 {
        part(b, "docProps/app.xml").split("<HiddenSlides>").nth(1).unwrap().split('<').next().unwrap().parse().unwrap()
    };
    let src = patch(&fixture("poi/bar-chart.pptx"), "ppt/slides/slide1.xml", |s| s.replacen("<p:sld ", "<p:sld show=\"0\" ", 1));
    let src = patch(&src, "docProps/app.xml", |s| s.replace("<HiddenSlides>0</HiddenSlides>", "<HiddenSlides>1</HiddenSlides>"));
    let mut p = Presentation::from_bytes(src).unwrap();
    p.delete_slide(0).unwrap();
    assert_eq!(count(&p.to_bytes().unwrap()), 0);
    let src = patch(&fixture("poi/sample.pptx"), "ppt/slides/slide1.xml", |s| s.replacen("<p:sld ", "<p:sld show=\"0\" ", 1));
    let before = count(&src);
    let mut p = Presentation::from_bytes(src).unwrap();
    p.duplicate_slide(0, None).unwrap();
    assert_eq!(count(&p.to_bytes().unwrap()), before + 1);
}
