//! Phase 1b tables (ADR-0013 §2, §3) on real files: locality (only the
//! table and the frame size change), `a16` ids, merge/unmerge with text,
//! comment locks, borders, fills, style ids, sizes, and refusals that leave
//! the package byte-identical.

use std::assert_matches;
use std::io::{Cursor, Read, Write};

use ooxml_pptx::{CellFill, Edge, Error, Line, Presentation, Side, TableColor, TableFlag, TableMut};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx");
const TT2: &str = "poi/table_test2.pptx";
const SLIDE: &str = "ppt/slides/slide1.xml";

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{rel}")).unwrap()
}

fn part(b: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
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

/// Runs `f` on the table of frame `id` on slide 0.
fn on_table<T>(p: &mut Presentation, id: u32, f: impl FnOnce(&mut TableMut<'_>) -> T) -> T {
    let mut s = p.slide_mut(0).unwrap();
    let mut sh = s.shape_mut_by_id(id).unwrap();
    let mut t = sh.table_mut().unwrap();
    f(&mut t)
}

fn edit(src: &[u8], f: impl FnOnce(&mut TableMut<'_>) -> Result<(), Error>) -> (Result<(), Error>, Vec<u8>) {
    let mut p = Presentation::from_bytes(src.to_vec()).unwrap();
    let r = on_table(&mut p, 4, f);
    (r, p.to_bytes().unwrap())
}

fn table_xml(slide: &str) -> &str {
    &slide[slide.find("<a:tbl>").unwrap()..slide.find("</a:tbl>").unwrap() + 8]
}

/// The slide outside `<a:tbl>…</a:tbl>` and the frame's `a:ext`.
fn outside(slide: &str) -> String {
    let t = table_xml(slide);
    let s = slide.replace(t, "<TABLE/>");
    let i = s.find("<p:xfrm>").unwrap();
    let j = s[i..].find("</p:xfrm>").unwrap() + i;
    format!("{}{}", &s[..i], &s[j..])
}

fn ids(t: &str, local: &str) -> Vec<u64> {
    t.split(&format!("<a16:{local} ")).skip(1).map(|x| x.split("val=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap()).collect()
}

/// The cells of each row: `(attributes of <a:tc>, whole cell XML)`.
fn grid(slide: &str) -> Vec<Vec<(String, String)>> {
    table_xml(slide)
        .split("<a:tr ")
        .skip(1)
        .map(|row| {
            let mut out = Vec::new();
            let mut rest = row;
            while let Some(i) = rest.find("<a:tc").filter(|_| true) {
                let after = &rest[i + 5..];
                if !(after.starts_with(' ') || after.starts_with('>')) {
                    rest = &rest[i + 5..];
                    continue;
                }
                let end = rest[i..].find("</a:tc>").map_or(rest.len(), |e| i + e + 7);
                let cell = &rest[i..end];
                let attrs = cell[5..cell.find('>').unwrap()].to_owned();
                out.push((attrs, cell.to_owned()));
                rest = &rest[end..];
            }
            out
        })
        .collect()
}

fn frame_ext(slide: &str) -> (i64, i64) {
    let x = &slide[slide.find("<p:xfrm>").unwrap()..];
    let e = &x[x.find("<a:ext ").unwrap()..];
    let g = |a: &str| e.split(&format!("{a}=\"")).nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
    (g("cx"), g("cy"))
}

/// Every other part keeps its compressed bytes.
fn others_unchanged(base: &[u8], out: &[u8]) {
    let a = raw_entries(base);
    let b = raw_entries(out);
    for (n, crc, v) in &a {
        if n == SLIDE {
            continue;
        }
        let w = b.iter().find(|x| &x.0 == n).unwrap();
        assert_eq!((crc, v), (&w.1, &w.2), "{n} changed");
    }
}

#[test]
fn insert_row_copies_format_and_gets_a_new_id() {
    let src = fixture(TT2);
    let base = unchanged(&src);
    let s0 = part(&src, SLIDE);
    let rows0 = ids(table_xml(&s0), "rowId");
    let (cx0, cy0) = frame_ext(&s0);
    let (r, out) = edit(&src, |t| t.insert_row(1));
    r.unwrap();
    others_unchanged(&base, &out);
    let s1 = part(&out, SLIDE);
    assert_eq!(outside(&s0), outside(&s1), "only the table and the frame size change");
    let rows1 = ids(table_xml(&s1), "rowId");
    assert_eq!(rows1.len(), rows0.len() + 1);
    assert_eq!(rows1[1], rows0.iter().max().unwrap() + 1, "max + 1");
    assert!(s1.contains(&format!("<a16:rowId xmlns:a16=\"http://schemas.microsoft.com/office/drawing/2014/main\" val=\"{}\"/>", rows1[1])));
    // the row above is 370840 high in this file; the frame grows by it
    let h0: i64 = table_xml(&s0).split("<a:tr h=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
    assert!(h0 > 0);
    assert_eq!(frame_ext(&s1), (cx0, cy0 + h0));
    // new cells copy the row above's merge-free formatting: row 0 has a
    // 1-D horizontal merge, which a new row outside every vertical merge
    // does not join; the cell under "Header2" copies its pPr (algn="ctr")
    let new_row = table_xml(&s1).split("<a:tr ").nth(2).unwrap();
    assert!(!new_row.contains("gridSpan") && !new_row.contains("hMerge"), "{new_row}");
    assert!(new_row.contains("<a:pPr algn=\"ctr\"/>"));
    Presentation::from_bytes(out).unwrap();
}

#[test]
fn deleted_ids_are_not_reused_in_the_session() {
    let src = fixture(TT2);
    let mut p = Presentation::from_bytes(src.clone()).unwrap();
    let rows0 = ids(table_xml(&part(&src, SLIDE)), "rowId");
    let cols0 = ids(table_xml(&part(&src, SLIDE)), "colId");
    let last_row = rows0.len() - 1;
    // the last row is plain (it is not covered by the rowSpan above it)
    on_table(&mut p, 4, |t| t.delete_row(last_row)).unwrap();
    on_table(&mut p, 4, |t| t.insert_row(0)).unwrap();
    on_table(&mut p, 4, |t| t.delete_column(5)).unwrap();
    on_table(&mut p, 4, |t| t.insert_column(5)).unwrap();
    let s = part(&p.to_bytes().unwrap(), SLIDE);
    let rows = ids(table_xml(&s), "rowId");
    let cols = ids(table_xml(&s), "colId");
    let rmax = *rows0.iter().max().unwrap();
    let cmax = *cols0.iter().max().unwrap();
    assert_eq!(rows[0], rmax.max(rows0[last_row]) + 1);
    assert!(!rows[..1].contains(&rows0[last_row]));
    assert!(cols.contains(&(cmax.max(cols0[5]) + 1)) && !cols.contains(&cols0[5]));
}

#[test]
fn merge_moves_text_and_unmerge_keeps_it() {
    let src = fixture(TT2);
    // rows 3–4, columns 1–2 are plain cells: merge the 2×2 square
    let s0 = part(&src, SLIDE);
    let (r, out) = edit(&src, |t| {
        assert_eq!(t.size(), (7, 6));
        t.merge(3, 1, 4, 2)
    });
    r.unwrap();
    let s1 = part(&out, SLIDE);
    assert_eq!(outside(&s0), outside(&s1));
    let g = grid(&s1);
    assert_eq!(g[3][1].0, " gridSpan=\"2\" rowSpan=\"2\"");
    assert_eq!(g[3][2].0, " rowSpan=\"2\" hMerge=\"1\"", "edge form");
    assert_eq!(g[4][1].0, " gridSpan=\"2\" vMerge=\"1\"");
    assert_eq!(g[4][2].0, " hMerge=\"1\" vMerge=\"1\"");
    Presentation::from_bytes(out.clone()).unwrap();
    // unmerge restores plain cells (text stays in the origin)
    let mut p = Presentation::from_bytes(out).unwrap();
    on_table(&mut p, 4, |t| t.unmerge(3, 1)).unwrap();
    let g = grid(&part(&p.to_bytes().unwrap(), SLIDE));
    for (r, c) in [(3, 1), (3, 2), (4, 1), (4, 2)] {
        assert_eq!(g[r][c].0, "", "({r},{c})");
    }
}

#[test]
fn merge_moves_paragraphs_in_row_major_order() {
    let src = fixture(TT2);
    let mut p = Presentation::from_bytes(src).unwrap();
    // give the four cells distinct text
    {
        let mut s = p.slide_mut(0).unwrap();
        let mut sh = s.shape_mut_by_id(4).unwrap();
        for (r, c, txt) in [(3, 1, ""), (3, 2, "b"), (4, 1, "c"), (4, 2, "")] {
            sh.table_cell_mut(r, c).unwrap().set_text(txt).unwrap();
        }
    }
    on_table(&mut p, 4, |t| t.merge(3, 1, 4, 2)).unwrap();
    let out = p.to_bytes().unwrap();
    Presentation::from_bytes(out.clone()).unwrap();
    let g = grid(&part(&out, SLIDE));
    let origin = &g[3][1].1;
    // the origin's single empty paragraph is replaced; b then c
    assert_eq!(origin.matches("<a:p>").count() + origin.matches("<a:p ").count(), 2, "{origin}");
    assert!(origin.find(">b<").unwrap() < origin.find(">c<").unwrap());
    // covered cells keep one empty paragraph
    for (r, c) in [(3, 2), (4, 1), (4, 2)] {
        let cell = &g[r][c].1;
        assert!(!cell.contains("<a:t>b") && !cell.contains("<a:t>c") && cell.contains("<a:p"), "({r},{c}): {cell}");
    }
}

#[test]
fn merge_refusals() {
    let src = fixture(TT2);
    for (f, want) in [
        ((0usize, 0usize, 0usize, 2usize), "contains"), // (0,1) is already a merge origin
        ((3, 1, 7, 2), "outside"),
        ((3, 1, 3, 1), "outside"),
    ] {
        let (r, out) = edit(&src, |t| t.merge(f.0, f.1, f.2, f.3));
        match want {
            "contains" => assert_matches!(r, Err(Error::RangeContainsMerge)),
            _ => assert_matches!(r, Err(Error::RangeOutsideGrid)),
        }
        assert_eq!(out, unchanged(&src));
    }
    let (r, out) = edit(&src, |t| t.unmerge(0, 0));
    assert_matches!(r, Err(Error::NotAMergeOrigin { row: 0, col: 0 }));
    assert_eq!(out, unchanged(&src));
}

#[test]
fn structural_refusals_change_nothing() {
    let src = fixture(TT2);
    let base = unchanged(&src);
    // row 3 holds rowSpan origins; column 1 holds gridSpan origins
    let (r, out) = edit(&src, |t| t.delete_row(3));
    assert_matches!(r, Err(Error::MergeOriginInRow { row: 3 }));
    assert_eq!(out, base);
    let (r, out) = edit(&src, |t| t.delete_column(1));
    assert_matches!(r, Err(Error::MergeOriginInColumn { col: 1 }));
    assert_eq!(out, base);
    // a short row is not well-formed
    let bad = patch(&src, SLIDE, |s| {
        let i = s.rfind("<a:tc>").unwrap();
        let j = i + s[i..].find("</a:tc>").unwrap() + 7;
        format!("{}{}", &s[..i], &s[j..])
    });
    let (r, out) = edit(&bad, |t| t.insert_row(0));
    assert_matches!(r, Err(Error::TableNotWellFormed(_)));
    assert_eq!(out, unchanged(&bad));
    // only some rows carry ids
    let partial = patch(&src, SLIDE, |s| {
        let i = s.find("<a:extLst><a:ext uri=\"{0D108BD9").unwrap();
        let j = i + s[i..].find("</a:extLst>").unwrap() + 11;
        format!("{}{}", &s[..i], &s[j..])
    });
    let (r, out) = edit(&partial, |t| t.insert_row(0));
    assert_matches!(r, Err(Error::TableNotWellFormed(_)));
    assert_eq!(out, unchanged(&partial));
    // a format that cannot be copied (a hyperlink in the source tcPr's
    // neighbour is fine; an extLst in a:tcPr is not)
    let ext = patch(&src, SLIDE, |s| s.replacen("<a:tcPr/>", "<a:tcPr><a:extLst><a:ext uri=\"{X}\"/></a:extLst></a:tcPr>", 1));
    let (r, out) = edit(&ext, |t| t.insert_row(1));
    assert_matches!(r, Err(Error::FormatNotCopyable("a:tcPr")));
    assert_eq!(out, unchanged(&ext));
    // the last row / column of a one-cell table
    let one = patch(&src, SLIDE, |s| {
        let i = s.find("<a:tbl>").unwrap();
        let j = s.find("</a:tbl>").unwrap() + 8;
        format!("{}<a:tbl><a:tblGrid><a:gridCol w=\"100\"/></a:tblGrid><a:tr h=\"10\"><a:tc><a:txBody><a:bodyPr/><a:p/></a:txBody></a:tc></a:tr></a:tbl>{}", &s[..i], &s[j..])
    });
    let (r, _) = edit(&one, |t| t.delete_row(0));
    assert_matches!(r, Err(Error::LastRow));
    let (r, _) = edit(&one, |t| t.delete_column(0));
    assert_matches!(r, Err(Error::LastColumn));
}

fn with_comment(src: &[u8], anchor: &str) -> Vec<u8> {
    let cm = format!(
        "<p188:cmLst xmlns:p188=\"http://schemas.microsoft.com/office/powerpoint/2018/8/main\" xmlns:ac=\"http://schemas.microsoft.com/office/drawing/2013/main/command\" xmlns:pc=\"http://schemas.microsoft.com/office/powerpoint/2013/main/command\"><p188:cm id=\"{{1}}\" authorId=\"{{2}}\">{anchor}<p188:txBody><a:bodyPr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"/></p188:txBody></p188:cm></p188:cmLst>"
    );
    let b = patch(src, "ppt/comments/modernComment_1.xml", |_| cm.clone());
    let b = patch(&b, "ppt/slides/_rels/slide1.xml.rels", |s| {
        s.replace("</Relationships>", "<Relationship Id=\"rId90\" Type=\"http://schemas.microsoft.com/office/2018/10/relationships/comments\" Target=\"../comments/modernComment_1.xml\"/></Relationships>")
    });
    patch(&b, "[Content_Types].xml", |s| {
        s.replace("</Types>", "<Override PartName=\"/ppt/comments/modernComment_1.xml\" ContentType=\"application/vnd.ms-powerpoint.comments+xml\"/></Types>")
    })
}

#[test]
fn comments_inside_the_table_lock_structure_only() {
    let src = fixture(TT2);
    for anchor in [
        "<ac:tcMkLst><pc:docMk/><pc:sldMk cId=\"0\" sldId=\"256\"/><ac:graphicFrameMk id=\"4\" creationId=\"{0}\"/><ac:tcMk rowId=\"1\" colId=\"2\"/></ac:tcMkLst>",
        // any list holding a row moniker, whatever its name
        "<ac:deMkLst><pc:docMk/><ac:graphicFrameMk id=\"4\" creationId=\"{0}\"/><ac:trMk rowId=\"1\"/></ac:deMkLst>",
        // an anchor that cannot be verified locks every table
        "<p188:unknownAnchor/>",
    ] {
        let b = with_comment(&src, anchor);
        let base = unchanged(&b);
        for op in 0..4 {
            let (r, out) = edit(&b, |t| match op {
                0 => t.insert_row(0),
                1 => t.delete_row(6),
                2 => t.insert_column(0),
                _ => t.merge(3, 1, 4, 2),
            });
            assert_matches!(r, Err(Error::CommentAnchoredTable { shape_id: 4 }), "{anchor} op {op}");
            assert_eq!(out, base);
        }
        // properties are allowed
        let (r, _) = edit(&b, |t| t.set_column_width(0, 1_000_000));
        r.unwrap();
    }
    // a whole-frame anchor does not lock structural edits
    let b = with_comment(&src, "<ac:deMkLst><pc:docMk/><pc:sldMk cId=\"0\" sldId=\"256\"/><ac:graphicFrameMk id=\"4\" creationId=\"{0}\"/></ac:deMkLst>");
    let (r, _) = edit(&b, |t| t.insert_row(0));
    r.unwrap();
}

#[test]
fn borders_write_both_sides_of_a_shared_edge() {
    let src = fixture(TT2);
    let line = Line::Solid { width: 12700, color: TableColor::Rgb([0x11, 0x22, 0x33]) };
    // (3, 2): a plain cell; its left edge pairs with (3, 1)'s right edge
    let (r, out) = edit(&src, |t| t.set_border(3, 2, Edge::Left, &line));
    r.unwrap();
    let g = grid(&part(&out, SLIDE));
    let ln = "w=\"12700\"><a:solidFill><a:srgbClr val=\"112233\"/></a:solidFill>";
    assert!(g[3][2].1.contains(&format!("<a:lnL {ln}")), "{}", g[3][2].1);
    assert!(g[3][1].1.contains(&format!("<a:lnR {ln}")), "{}", g[3][1].1);
    // a merged range: (3, 0) has rowSpan=2; asking on its covered cell
    // (4, 0) means the range's outline, and the neighbours across it
    let (r, out) = edit(&src, |t| t.set_border(4, 0, Edge::Right, &Line::None));
    r.unwrap();
    let g = grid(&part(&out, SLIDE));
    for (rr, row) in g.iter().enumerate().take(5).skip(3) {
        assert!(row[0].1.contains("<a:lnR><a:noFill/></a:lnR>"), "row {rr}: {}", row[0].1);
        assert!(row[1].1.contains("<a:lnL><a:noFill/></a:lnL>"), "row {rr}: {}", row[1].1);
    }
    for rr in [2, 5] {
        assert!(!g[rr][0].1.contains("<a:lnR>"), "row {rr} is outside the range");
    }
    // out of range values are refused before any change
    let (r, out) = edit(&src, |t| t.set_border(0, 0, Edge::Top, &Line::Solid { width: -1, color: TableColor::Rgb([0, 0, 0]) }));
    assert_matches!(r, Err(Error::ValueOutOfRange { .. }));
    assert_eq!(out, unchanged(&src));
    let (r, _) = edit(&src, |t| t.set_border(0, 0, Edge::Top, &Line::Solid { width: 1, color: TableColor::Scheme("nope".into()) }));
    assert_matches!(r, Err(Error::ValueOutOfRange { .. }));
}

#[test]
fn fills_margins_anchor_flags() {
    let src = fixture(TT2);
    let (r, out) = edit(&src, |t| {
        t.set_fill(0, 0, &CellFill::Solid(TableColor::Scheme("accent1".into())))?;
        t.set_fill(0, 0, &CellFill::None)?;
        t.set_margin(0, 0, Side::Left, Some(91440))?;
        t.set_anchor(0, 0, Some("ctr"))?;
        t.set_flag(TableFlag::BandColumns, true)?;
        t.set_flag(TableFlag::FirstRow, false)
    });
    r.unwrap();
    let s = part(&out, SLIDE);
    let t = table_xml(&s);
    let first = grid(&s)[0][0].1.clone();
    assert!(first.contains("<a:tcPr marL=\"91440\" anchor=\"ctr\"><a:noFill/></a:tcPr>"), "{first}");
    assert!(t.starts_with("<a:tbl><a:tblPr bandRow=\"1\" bandCol=\"1\">"), "{}", &t[..80]);
}

#[test]
fn style_ids_defined_or_built_in() {
    let src = fixture(TT2);
    // a built-in id, case-insensitively
    let (r, out) = edit(&src, |t| t.set_style(Some("{073a0daa-6af3-43ab-8588-cec1d06c72b9}")));
    r.unwrap();
    // written in the built-in list's canonical spelling (ST_Guid: uppercase; review #57 M1)
    assert!(part(&out, SLIDE).contains("<a:tableStyleId>{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}</a:tableStyleId>"));
    let (r, out) = edit(&src, |t| t.set_style(Some(" {073a0daa-6af3-43ab-8588-cec1d06c72b9} ")));
    r.unwrap();
    assert!(part(&out, SLIDE).contains("<a:tableStyleId>{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}</a:tableStyleId>"));
    // the same style in another spelling changes nothing
    let (r, out) = edit(&src, |t| t.set_style(Some("{5c22544a-7ee6-4342-b048-85bdc9fd1c3a}")));
    r.unwrap();
    assert_eq!(out, unchanged(&src));
    let (r, out) = edit(&src, |t| t.set_style(Some("{00000000-0000-0000-0000-000000000000}")));
    assert_matches!(r, Err(Error::TableStyleNotDefined(_)));
    assert_eq!(out, unchanged(&src));
    let (r, out) = edit(&src, |t| t.set_style(None));
    r.unwrap();
    assert!(!part(&out, SLIDE).contains("tableStyleId"));
    // a style defined in tableStyles.xml
    let defined = patch(&src, "ppt/tableStyles.xml", |s| {
        s.replace("</a:tblStyleLst>", "<a:tblStyle styleId=\"{11111111-2222-3333-4444-555555555555}\" styleName=\"x\"/></a:tblStyleLst>")
    });
    let (r, _) = edit(&defined, |t| t.set_style(Some("{11111111-2222-3333-4444-555555555555}")));
    r.unwrap();
}

#[test]
fn sizes_follow_the_frame() {
    let src = fixture(TT2);
    let s0 = part(&src, SLIDE);
    let (cx0, cy0) = frame_ext(&s0);
    let (r, out) = edit(&src, |t| {
        t.set_column_width(0, 2_032_000 + 500)?;
        let h0 = 370_840; // replaced below by the file's value
        let _ = h0;
        Ok(())
    });
    r.unwrap();
    let h0: i64 = table_xml(&s0).split("<a:tr h=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
    let mut p = Presentation::from_bytes(out).unwrap();
    let r = on_table(&mut p, 4, |t| t.set_row_height(0, h0 + 7));
    let out = p.to_bytes().unwrap();
    r.unwrap();
    assert_eq!(frame_ext(&part(&out, SLIDE)), (cx0 + 500, cy0 + 7));
    // unchanged values change nothing
    let (r, out) = edit(&src, |t| t.set_column_width(0, 2_032_000));
    r.unwrap();
    assert_eq!(out, unchanged(&src));
    let (r, _) = edit(&src, |t| t.set_column_width(0, -1));
    assert_matches!(r, Err(Error::ValueOutOfRange { .. }));
}

#[test]
fn an_id_deleted_as_the_maximum_is_still_skipped() {
    // the last row gets the largest id; deleting it and inserting a row
    // must not hand out that id again (ADR-0013 §2)
    let src = patch(&fixture(TT2), SLIDE, |s| s.replace("val=\"949750401\"", "val=\"4294967000\""));
    let mut p = Presentation::from_bytes(src).unwrap();
    on_table(&mut p, 4, |t| t.delete_row(6)).unwrap();
    on_table(&mut p, 4, |t| t.insert_row(0)).unwrap();
    let rows = ids(table_xml(&part(&p.to_bytes().unwrap(), SLIDE)), "rowId");
    assert_eq!(rows[0], 4_294_967_001);
}

#[test]
fn any_row_or_column_id_under_an_anchor_locks() {
    let src = fixture(TT2);
    // an unknown moniker name, but it names a row: locked
    let b = with_comment(&src, "<ac:deMkLst><pc:docMk/><ac:graphicFrameMk id=\"4\" creationId=\"{0}\"/><ac:newMk rowId=\"1\"/></ac:deMkLst>");
    let (r, _) = edit(&b, |t| t.insert_row(0));
    assert_matches!(r, Err(Error::CommentAnchoredTable { shape_id: 4 }));
    let b = with_comment(&src, "<ac:deMkLst><pc:docMk/><ac:graphicFrameMk id=\"4\" creationId=\"{0}\"/><ac:newMk colId=\"1\"/></ac:deMkLst>");
    let (r, _) = edit(&b, |t| t.insert_column(0));
    assert_matches!(r, Err(Error::CommentAnchoredTable { shape_id: 4 }));
}

#[test]
fn a_merge_mixing_edge_spans_and_flags_is_not_well_formed() {
    // (2, 3) has rowSpan=3 over three columns: make it a 2-D merge whose
    // origin-column cells carry gridSpan but one origin-row cell lacks
    // its rowSpan
    let src = patch(&fixture(TT2), SLIDE, |s| {
        let t0 = s.find("<a:tbl>").unwrap();
        let t1 = s.find("</a:tbl>").unwrap();
        let mut rows: Vec<String> = s[t0..t1].split("<a:tr ").map(str::to_owned).collect();
        // rows[k + 1] is table row k: row 2 holds the origins, rows 3–4 the covered cells
        let r2 = &mut rows[3];
        *r2 = r2.replacen("<a:tc rowSpan=\"3\">", "<a:tc gridSpan=\"3\" rowSpan=\"3\">", 1);
        *r2 = r2.replacen("<a:tc rowSpan=\"3\">", "<a:tc rowSpan=\"3\" hMerge=\"1\">", 1);
        *r2 = r2.replacen("<a:tc rowSpan=\"3\">", "<a:tc hMerge=\"1\">", 1); // edge span missing
        for k in [4, 5] {
            let r = &mut rows[k];
            let first = r.match_indices("<a:tc vMerge=\"1\">").map(|(i, _)| i).last().unwrap();
            // the last three vMerge cells are columns 3–5
            let idx: Vec<usize> = r.match_indices("<a:tc vMerge=\"1\">").map(|(i, _)| i).collect();
            let start = idx[idx.len() - 3];
            let _ = first;
            let (a, b) = r.split_at(start);
            let b = b
                .replacen("<a:tc vMerge=\"1\">", "<a:tc gridSpan=\"3\" vMerge=\"X\">", 1)
                .replacen("<a:tc vMerge=\"1\">", "<a:tc hMerge=\"1\" vMerge=\"X\">", 1)
                .replacen("<a:tc vMerge=\"1\">", "<a:tc hMerge=\"1\" vMerge=\"X\">", 1)
                .replace("vMerge=\"X\"", "vMerge=\"1\"");
            *r = format!("{a}{b}");
        }
        format!("{}{}{}", &s[..t0], rows.join("<a:tr "), &s[t1..])
    });
    let (r, out) = edit(&src, |t| t.insert_row(0));
    assert_matches!(r, Err(Error::TableNotWellFormed(ref m)) if m.contains("mixes"), "{r:?}");
    assert_eq!(out, unchanged(&src));
}


// ------------------------------------------------------------------ review #57 round 1

#[test]
fn a_defined_style_is_written_in_the_definitions_spelling() {
    let src = patch(&fixture(TT2), "ppt/tableStyles.xml", |s| {
        s.replace("</a:tblStyleLst>", "<a:tblStyle styleId=\"{ABCDEF01-2222-3333-4444-555555555555}\" styleName=\"x\"/></a:tblStyleLst>")
    });
    let (r, out) = edit(&src, |t| t.set_style(Some("{abcdef01-2222-3333-4444-555555555555}")));
    r.unwrap();
    assert!(part(&out, SLIDE).contains("<a:tableStyleId>{ABCDEF01-2222-3333-4444-555555555555}</a:tableStyleId>"));
}

#[test]
fn an_inline_table_style_is_refused() {
    let src = patch(&fixture(TT2), SLIDE, |s| {
        let i = s.find("<a:tableStyleId>").unwrap();
        let j = s.find("</a:tableStyleId>").unwrap() + "</a:tableStyleId>".len();
        format!("{}<a:tableStyle styleId=\"{{11111111-1111-1111-1111-111111111111}}\" styleName=\"inline\"/>{}", &s[..i], &s[j..])
    });
    for id in [Some("{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}"), None] {
        let (r, out) = edit(&src, |t| t.set_style(id));
        assert_matches!(r, Err(Error::TableStyleInline), "{id:?}");
        assert_eq!(out, unchanged(&src));
    }
}

#[test]
fn merge_works_when_the_origin_has_no_text_body() {
    // no txBody on the origin: nothing to move is fine, and moved text gets a body
    let strip = |src: &[u8], r: usize, c: usize| {
        patch(src, SLIDE, |s| {
            let g = grid(&s);
            let cell = &g[r][c].1;
            let i = cell.find("<a:txBody>").unwrap();
            let j = cell.find("</a:txBody>").unwrap() + "</a:txBody>".len();
            s.replacen(cell.as_str(), &format!("{}{}", &cell[..i], &cell[j..]), 1)
        })
    };
    // empty the three other cells first: then nothing moves
    let mut p = Presentation::from_bytes(fixture(TT2)).unwrap();
    {
        let mut s = p.slide_mut(0).unwrap();
        let mut sh = s.shape_mut_by_id(4).unwrap();
        for (r, c) in [(3, 2), (4, 1), (4, 2)] {
            sh.table_cell_mut(r, c).unwrap().set_text("").unwrap();
        }
    }
    let src = strip(&p.to_bytes().unwrap(), 3, 1);
    let (r, out) = edit(&src, |t| t.merge(3, 1, 4, 2));
    r.unwrap();
    assert!(!grid(&part(&out, SLIDE))[3][1].1.contains("<a:txBody>"));
    // with text in (3,2) the origin gets a body holding it
    let mut p = Presentation::from_bytes(src).unwrap();
    {
        let mut s = p.slide_mut(0).unwrap();
        let mut sh = s.shape_mut_by_id(4).unwrap();
        sh.table_cell_mut(3, 2).unwrap().set_text("moved").unwrap();
    }
    on_table(&mut p, 4, |t| t.merge(3, 1, 4, 2)).unwrap();
    let out = p.to_bytes().unwrap();
    let origin = grid(&part(&out, SLIDE))[3][1].1.clone();
    assert!(origin.contains("<a:txBody><a:bodyPr/>") && origin.contains(">moved<"), "{origin}");
    assert_eq!(origin.matches("<a:p>").count() + origin.matches("<a:p ").count(), 1, "{origin}");
}

#[test]
fn an_anchor_on_another_shape_does_not_lock_the_table() {
    let src = fixture(TT2);
    let b = with_comment(&src, "<ac:spMkLst><pc:docMk/><ac:spMk id=\"99\" creationId=\"{0}\"/></ac:spMkLst>");
    let (r, _) = edit(&b, |t| t.insert_row(0));
    r.unwrap();
    // naming the table's own id locks it
    let b = with_comment(&src, "<ac:spMkLst><pc:docMk/><ac:spMk id=\"4\" creationId=\"{0}\"/></ac:spMkLst>");
    let (r, _) = edit(&b, |t| t.insert_row(0));
    assert_matches!(r, Err(Error::CommentAnchoredTable { shape_id: 4 }));
}

#[test]
fn duplicate_ids_are_not_well_formed() {
    let src = patch(&fixture(TT2), SLIDE, |s| s.replace("val=\"949750401\"", "val=\"355537859\""));
    let (r, out) = edit(&src, |t| t.insert_row(0));
    assert_matches!(r, Err(Error::TableNotWellFormed(ref m)) if m.contains("duplicate"), "{r:?}");
    assert_eq!(out, unchanged(&src));
}

#[test]
fn deleting_shrinks_the_frame() {
    let src = fixture(TT2);
    let s0 = part(&src, SLIDE);
    let (cx0, cy0) = frame_ext(&s0);
    let rows = grid(&s0);
    let _ = rows;
    let h6: i64 = table_xml(&s0).split("<a:tr h=\"").nth(7).unwrap().split('"').next().unwrap().parse().unwrap();
    let w5: i64 = table_xml(&s0).split("<a:gridCol w=\"").nth(6).unwrap().split('"').next().unwrap().parse().unwrap();
    let (r, out) = edit(&src, |t| {
        t.delete_row(6)?;
        t.delete_column(5)
    });
    r.unwrap();
    assert_eq!(frame_ext(&part(&out, SLIDE)), (cx0 - w5, cy0 - h6));
}

#[test]
fn insert_column_copies_width_and_format_from_the_left() {
    // column 0 and column 1 differ: give column 1 a distinct width and tcPr
    let src = patch(&fixture(TT2), SLIDE, |s| {
        let g = grid(&s);
        let cell = g[6][1].1.clone();
        s.replacen(&cell, &cell.replacen("<a:tcPr/>", "<a:tcPr marL=\"12345\"/>", 1), 1)
    });
    let s0 = part(&src, SLIDE);
    let w1: String = table_xml(&s0).split("<a:gridCol w=\"").nth(2).unwrap().split('"').next().unwrap().to_owned();
    let (r, out) = edit(&src, |t| t.insert_column(2));
    r.unwrap();
    let s1 = part(&out, SLIDE);
    let w_new: String = table_xml(&s1).split("<a:gridCol w=\"").nth(3).unwrap().split('"').next().unwrap().to_owned();
    assert_eq!(w_new, w1, "width from the left column");
    assert!(grid(&s1)[6][2].1.contains("<a:tcPr marL=\"12345\"/>"), "{}", grid(&s1)[6][2].1);
}

#[test]
fn indexed_table_edits_refuse_same_slot_alternate_content() {
    let ac = |child: &str| {
        format!(r#"<mc:AlternateContent xmlns:mc="{}"><mc:Choice Requires="a">{child}</mc:Choice></mc:AlternateContent>"#,
                ooxml_core::name::MC_NS)
    };
    type Edit = fn(&mut TableMut<'_>) -> Result<(), Error>;
    let cases: [(&str, &str, Edit); 3] = [
        ("</a:tbl>", r#"<a:tr h="10"/>"#, |t| t.insert_row(0)),
        ("</a:tblGrid>", r#"<a:gridCol w="10"/>"#, |t| t.insert_column(0)),
        ("</a:tr>", "<a:tc/>", |t| t.insert_column(0)),
    ];
    for (close, child, op) in cases {
        let input = patch(&fixture(TT2), SLIDE, |xml| xml.replacen(close, &format!("{}{close}", ac(child)), 1));
        let (result, output) = edit(&input, op);
        assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{close}");
        assert_eq!(output, unchanged(&input));
    }
}

#[test]
fn table_merge_move_dry_run_refuses_same_slot_alternate_content() {
    let input = patch(&fixture(TT2), SLIDE, |xml| {
        let i = xml.find("<a:tbl>").unwrap();
        let j = i + xml[i..].find("</a:tbl>").unwrap() + 8;
        let ac = format!(r#"<mc:AlternateContent xmlns:mc="{}"><mc:Choice Requires="a"><a:p/></mc:Choice></mc:AlternateContent>"#,
                         ooxml_core::name::MC_NS);
        format!(r#"{}<a:tbl><a:tblGrid><a:gridCol w="100"/><a:gridCol w="100"/></a:tblGrid><a:tr h="10"><a:tc><a:txBody><a:bodyPr/><a:p/>{ac}</a:txBody></a:tc><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>moved</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl>{}"#,
                &xml[..i], &xml[j..])
    });
    let (result, output) = edit(&input, |t| t.merge(0, 0, 0, 1));
    assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(output, unchanged(&input));
}
