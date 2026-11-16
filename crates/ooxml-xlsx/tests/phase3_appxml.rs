//! ADR-0009 §11: `docProps/app.xml` follows every structural operation when
//! its shape is recognized, and is left untouched otherwise.

mod common;

use common::*;
use ooxml_core::XmlDocument;
use ooxml_xlsx::{SheetState, Workbook};

/// `(label, titles)` blocks of app.xml, read independently of the crate.
fn app_blocks(bytes: &[u8]) -> Option<Vec<(String, Vec<String>)>> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).ok()?;
    let part = pkg.part_names().find(|p| p.eq_ignore_ascii_case("docProps/app.xml"))?.to_owned();
    let doc = XmlDocument::parse(&pkg.read_part(&part).ok()?).ok()?;
    let child = |n: &ooxml_core::RawNode, l: &str| n.elements().find(|e| e.name().local() == l).cloned();
    let hp = child(doc.root(), "HeadingPairs").and_then(|h| child(&h, "vector"))?;
    let tp = child(doc.root(), "TitlesOfParts").and_then(|h| child(&h, "vector"))?;
    let titles: Vec<String> = tp.elements().map(|e| e.text().into_owned()).collect();
    if tp.attr(ooxml_core::QName::new("", "size")).and_then(|s| s.parse::<usize>().ok()) != Some(titles.len()) {
        return None;
    }
    let vars: Vec<&ooxml_core::RawNode> = hp.elements().collect();
    if hp.attr(ooxml_core::QName::new("", "size")).and_then(|s| s.parse::<usize>().ok()) != Some(vars.len()) || !vars.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 0;
    for p in vars.chunks(2) {
        let label = p[0].elements().next()?.text().into_owned();
        let n: usize = p[1].elements().next()?.text().trim().parse().ok()?;
        out.push((label, titles.get(at..at + n)?.to_vec()));
        at += n;
    }
    (at == titles.len()).then_some(out)
}

/// The worksheets app.xml should list: not `veryHidden`, workbook order.
fn expected_worksheets(bytes: &[u8]) -> Vec<String> {
    let wb = Workbook::from_bytes(bytes.to_vec()).unwrap();
    let part = wb.workbook_part().to_owned();
    let doc = XmlDocument::parse(&wb.package().read_part(&part).unwrap()).unwrap();
    let sheets = doc.root().elements().find(|e| e.name().local() == "sheets").unwrap();
    let rels = wb.package().relationships(&part).unwrap();
    // names as the crate reads them (ST_Xstring decoded), in workbook order
    let shown: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    sheets
        .elements()
        .zip(shown)
        .filter(|(s, _)| s.attr(ooxml_core::QName::new("", "state")) != Some("veryHidden"))
        .filter(|(s, _)| {
            let id = s.attrs().iter().find(|a| a.name().local() == "id").map(|a| a.value().to_owned()).unwrap_or_default();
            rels.by_id(&id).is_some_and(|r| r.rel_type.ends_with("/worksheet"))
        })
        .map(|(_, n)| n)
        .collect()
}

/// Recognized and consistent: the first block lists the worksheets.
fn consistent(bytes: &[u8]) -> bool {
    app_blocks(bytes).is_some_and(|b| b.first().is_some_and(|(_, t)| *t == expected_worksheets(bytes)))
}

fn app(bytes: &[u8]) -> Option<Vec<u8>> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).ok()?;
    let part = pkg.part_names().find(|p| p.eq_ignore_ascii_case("docProps/app.xml"))?.to_owned();
    pkg.read_part(&part).ok()
}

/// Every operation on every fixture: a consistent app.xml stays
/// consistent; any other app.xml is byte-for-byte untouched.
#[test]
fn app_xml_follows_every_operation() {
    let (checked, kept) = sweep_app(all_zip_fixtures().into_iter().map(|n| (n.clone(), read(&n))).collect());
    eprintln!("checked {checked} operations on recognized app.xml, {kept} on untouched ones");
    assert!(checked > 100, "{checked}");
}

/// The same over a directory of third-party files (`XLSX_CORPUS=dir`; not
/// committed, e.g. Apache POI test-data).
#[test]
#[ignore = "needs XLSX_CORPUS"]
fn app_xml_follows_every_operation_corpus() {
    let Ok(dir) = std::env::var("XLSX_CORPUS") else { return };
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "xlsx" || x == "xlsm"))
        .filter_map(|p| std::fs::read(&p).ok().map(|b| (p.display().to_string(), b)))
        .collect();
    files.sort();
    let (checked, kept) = sweep_app(files);
    eprintln!("corpus: checked {checked} operations on recognized app.xml, {kept} on untouched ones");
}

fn sweep_app(files: Vec<(String, Vec<u8>)>) -> (usize, usize) {
    let (mut checked, mut kept) = (0, 0);
    for (name, input) in files {
        let Ok(wb) = Workbook::from_bytes(input.clone()) else { continue };
        if wb.edit_refusal().is_some() {
            continue;
        }
        let sheets: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
        let n = sheets.len();
        let was = consistent(&input);
        type Op<'a> = Box<dyn Fn(&mut Workbook) -> ooxml_xlsx::Result<()> + 'a>;
        let mut ops: Vec<(String, Op<'_>)> = vec![
            ("add front".into(), Box::new(|w| w.add_sheet("New 1", 0))),
            ("add end".into(), Box::new(move |w| w.add_sheet("New 1", n))),
            ("reverse".into(), Box::new(move |w| (0..n).try_for_each(|i| w.move_sheet(n - 1, i)))),
        ];
        for (i, s) in sheets.iter().enumerate() {
            let s1 = s.clone();
            ops.push((format!("rename {s}"), Box::new(move |w| w.rename_sheet(&s1, &format!("Renamed {i}")).map(drop))));
            let s2 = s.clone();
            ops.push((format!("delete {s}"), Box::new(move |w| w.delete_sheet(&s2).map(drop))));
            let s3 = s.clone();
            ops.push((format!("very hidden {s}"), Box::new(move |w| w.set_sheet_state(&s3, SheetState::VeryHidden))));
        }
        for (label, op) in ops {
            let mut w = Workbook::from_bytes(input.clone()).unwrap();
            if op(&mut w).is_err() {
                continue;
            }
            let out = w.to_bytes().unwrap();
            if was {
                checked += 1;
                assert!(consistent(&out), "{name} {label}: app.xml is stale: {:?} vs {:?}", app_blocks(&out), expected_worksheets(&out));
            } else {
                kept += 1;
                assert_eq!(app(&input), app(&out), "{name} {label}: an unrecognized app.xml must stay untouched");
            }
        }
    }
    (checked, kept)
}

#[test]
fn template_app_xml_after_each_operation() {
    let titles = |b: &[u8]| app_blocks(b).unwrap();
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet("Data", 1).unwrap();
    wb.add_sheet("Front", 0).unwrap();
    assert_eq!(titles(&wb.to_bytes().unwrap()), vec![("Worksheets".to_owned(), vec!["Front".to_owned(), "Sheet1".into(), "Data".into()])]);
    wb.move_sheet(2, 0).unwrap();
    wb.rename_sheet("Sheet1", "Mid & <x>").unwrap();
    wb.set_sheet_state("Front", SheetState::VeryHidden).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(titles(&out), vec![("Worksheets".to_owned(), vec!["Data".to_owned(), "Mid & <x>".into()])]);
    assert!(String::from_utf8(app(&out).unwrap()).unwrap().contains("<vt:lpstr>Mid &amp; &lt;x&gt;</vt:lpstr>"));
    wb.set_sheet_state("Front", SheetState::Hidden).unwrap();
    wb.delete_sheet("Data").unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(titles(&out), vec![("Worksheets".to_owned(), vec!["Front".to_owned(), "Mid & <x>".into()])], "hidden (not very hidden) sheets are listed");
    assert!(consistent(&out));
}

const APP_NAMED: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><HeadingPairs><vt:vector size="4" baseType="variant"><vt:variant><vt:lpstr>Arbeitsblätter</vt:lpstr></vt:variant><vt:variant><vt:i4>2</vt:i4></vt:variant><vt:variant><vt:lpstr>Benannte Bereiche</vt:lpstr></vt:variant><vt:variant><vt:i4>2</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts><vt:vector size="4" baseType="lpstr"><vt:lpstr>A</vt:lpstr><vt:lpstr>B b</vt:lpstr><vt:lpstr>A!Print_Area</vt:lpstr><vt:lpstr>'B b'!Print_Titles</vt:lpstr></vt:vector></TitlesOfParts></Properties>"#;

fn with_app(input: &[u8], app_xml: &str) -> Vec<u8> {
    let ct = text(input, "[Content_Types].xml").replace(
        "</Types>",
        r#"<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#,
    );
    let rels = text(input, "_rels/.rels").replace(
        "</Relationships>",
        r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#,
    );
    rebuild(input, &[("[Content_Types].xml", ct.into_bytes()), ("_rels/.rels", rels.into_bytes()), ("docProps/app.xml", app_xml.as_bytes().to_vec())])
}

/// Localized labels are kept; Named Ranges titles follow a rename and lose
/// a deleted sheet's names.
#[test]
fn named_ranges_follow_rename_and_delete() {
    let base = common::p1::book(&[("A", ""), ("B b", "")], "", "");
    let input = with_app(&base, APP_NAMED);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.rename_sheet("B b", "Bee").unwrap();
    let b = app_blocks(&wb.to_bytes().unwrap()).unwrap();
    assert_eq!(b[0], ("Arbeitsblätter".to_owned(), vec!["A".to_owned(), "Bee".into()]));
    assert_eq!(b[1], ("Benannte Bereiche".to_owned(), vec!["A!Print_Area".to_owned(), "Bee!Print_Titles".into()]));
    wb.delete_sheet("A").unwrap();
    let b = app_blocks(&wb.to_bytes().unwrap()).unwrap();
    assert_eq!(b[0].1, vec!["Bee".to_owned()]);
    assert_eq!(b[1].1, vec!["Bee!Print_Titles".to_owned()]);
}

/// An app.xml whose shape is not recognized (counts that do not add up, or
/// a first block that is not the worksheets) is left untouched.
#[test]
fn unrecognized_app_xml_is_untouched() {
    let base = common::p1::book(&[("A", ""), ("B b", "")], "", "");
    for bad in [APP_NAMED.replace("<vt:i4>2</vt:i4></vt:variant></vt:vector>", "<vt:i4>3</vt:i4></vt:variant></vt:vector>"), APP_NAMED.replace("<vt:lpstr>B b</vt:lpstr>", "<vt:lpstr>Stale</vt:lpstr>")] {
        let input = with_app(&base, &bad);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.add_sheet("C", 2).unwrap();
        assert_eq!(app(&input), app(&wb.to_bytes().unwrap()));
    }
}

/// A workbook with worksheet `A` and chart sheet `C` (state as given), and
/// an app.xml with the given blocks.
fn with_chart_sheet(state: &str, app_xml: &str) -> Vec<u8> {
    let base = common::p1::book(&[("A", "")], "", "");
    let wb = text(&base, "xl/workbook.xml").replace("</sheets>", &format!(r#"<sheet name="C" sheetId="9"{state} r:id="rId9"/></sheets>"#));
    let rels = text(&base, "xl/_rels/workbook.xml.rels").replace(
        "</Relationships>",
        r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet" Target="chartsheets/sheet1.xml"/></Relationships>"#,
    );
    let ct = text(&base, "[Content_Types].xml").replace(
        "</Types>",
        r#"<Override PartName="/xl/chartsheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml"/></Types>"#,
    );
    let input = rebuild(&base, &[
        ("xl/workbook.xml", wb.into_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.into_bytes()),
        ("[Content_Types].xml", ct.into_bytes()),
        ("xl/chartsheets/sheet1.xml", br#"<chartsheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#.to_vec()),
    ]);
    with_app(&input, app_xml)
}

fn app_of(blocks: &[(&str, &[&str])]) -> String {
    let mut hp = String::new();
    let mut tp = String::new();
    let mut n = 0;
    for (label, titles) in blocks {
        hp.push_str(&format!("<vt:variant><vt:lpstr>{label}</vt:lpstr></vt:variant><vt:variant><vt:i4>{}</vt:i4></vt:variant>", titles.len()));
        for t in *titles {
            tp.push_str(&format!("<vt:lpstr>{t}</vt:lpstr>"));
            n += 1;
        }
    }
    format!(
        r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><HeadingPairs><vt:vector size="{}" baseType="variant">{hp}</vt:vector></HeadingPairs><TitlesOfParts><vt:vector size="{n}" baseType="lpstr">{tp}</vt:vector></TitlesOfParts></Properties>"#,
        2 * blocks.len()
    )
}

/// A chart-sheet block follows a rename of the chart sheet.
#[test]
fn chart_sheet_block_follows() {
    let input = with_chart_sheet("", &app_of(&[("Worksheets", &["A"]), ("Charts", &["C"])]));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.rename_sheet("C", "Chart 1").unwrap();
    let b = app_blocks(&wb.to_bytes().unwrap()).unwrap();
    assert_eq!(b[1], ("Charts".to_owned(), vec!["Chart 1".to_owned()]));
}

/// Unhiding a very hidden chart sheet would need a "Charts" block that does
/// not exist: app.xml is left untouched (never invent structure).
#[test]
fn a_missing_block_leaves_app_xml_untouched() {
    let input = with_chart_sheet(r#" state="veryHidden""#, &app_of(&[("Worksheets", &["A"])]));
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.set_sheet_state("C", SheetState::Visible).unwrap();
    assert_eq!(app(&input), app(&wb.to_bytes().unwrap()));
}

/// Counts that add up to fewer titles than listed: not recognized.
#[test]
fn counts_short_of_the_titles_are_not_recognized() {
    let base = common::p1::book(&[("A", ""), ("B b", "")], "", "");
    let bad = APP_NAMED.replace("<vt:i4>2</vt:i4></vt:variant></vt:vector>", "<vt:i4>1</vt:i4></vt:variant></vt:vector>");
    let input = with_app(&base, &bad);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.add_sheet("C", 2).unwrap();
    assert_eq!(app(&input), app(&wb.to_bytes().unwrap()));
}

// ---- PR #55 review round 1 (docs/reviews/pr55-round1.md) ----

/// m1: a block an operation empties goes with its pair (no count-0 pair);
/// unhiding afterwards finds no block and leaves app.xml untouched.
#[test]
fn an_emptied_block_is_removed() {
    let input = with_chart_sheet("", &app_of(&[("Worksheets", &["A"]), ("Charts", &["C"])]));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.set_sheet_state("C", SheetState::VeryHidden).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(app_blocks(&out).unwrap(), vec![("Worksheets".to_owned(), vec!["A".to_owned()])]);
    assert!(String::from_utf8(app(&out).unwrap()).unwrap().contains(r#"<vt:vector size="2" baseType="variant">"#));
    wb.set_sheet_state("C", SheetState::Visible).unwrap();
    assert_eq!(app(&out), app(&wb.to_bytes().unwrap()));
    // Named Ranges emptied by a delete
    let base = common::p1::book(&[("A", ""), ("B", "")], "", "");
    let input = with_app(&base, &app_of(&[("Worksheets", &["A", "B"]), ("Named Ranges", &["B!Print_Area"])]));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.delete_sheet("B").unwrap();
    assert_eq!(app_blocks(&wb.to_bytes().unwrap()).unwrap(), vec![("Worksheets".to_owned(), vec!["A".to_owned()])]);
}

/// V4: a count-0 block already in the input is not a kind's block: unhiding
/// the chart sheet does not fill it.
#[test]
fn an_existing_empty_block_is_not_filled() {
    let input = with_chart_sheet(r#" state="veryHidden""#, &app_of(&[("Worksheets", &["A"]), ("Charts", &[])]));
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.set_sheet_state("C", SheetState::Visible).unwrap();
    assert_eq!(app(&input), app(&wb.to_bytes().unwrap()));
}

/// m2 / V10: titles are plain text, not ST_Xstring.
#[test]
fn titles_are_plain_text() {
    let base = common::p1::book(&[("A", ""), ("B", "")], "", "");
    let input = with_app(&base, &app_of(&[("Worksheets", &["A", "B"])]));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.rename_sheet("A", "x_x0041_y").unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(app_blocks(&out).unwrap()[0].1, vec!["x_x0041_y".to_owned(), "B".into()]);
    // and the next operation still recognizes it
    let mut wb = Workbook::from_bytes(out).unwrap();
    wb.add_sheet("C", 2).unwrap();
    assert_eq!(app_blocks(&wb.to_bytes().unwrap()).unwrap()[0].1, vec!["x_x0041_y".to_owned(), "B".into(), "C".into()]);
}

/// m3 / V9, V16, V17: size, baseType and the children's types are part of
/// recognition; anything else is left untouched.
#[test]
fn vector_shape_is_part_of_recognition() {
    let base = common::p1::book(&[("A", ""), ("B", "")], "", "");
    let good = app_of(&[("Worksheets", &["A", "B"])]);
    let bad = [
        good.replace(r#"size="2" baseType="lpstr""#, r#"size="5" baseType="lpstr""#),
        good.replace(r#"size="2" baseType="variant""#, r#"size="6" baseType="variant""#),
        good.replace(r#"<vt:vector size="2" baseType="lpstr">"#, r#"<vt:vector baseType="lpstr">"#),
        good.replace(r#"baseType="lpstr""#, r#"baseType="variant""#),
        good.replace("<vt:lpstr>B</vt:lpstr>", "<vt:lpwstr>B</vt:lpwstr>"),
        good.replace("<vt:i4>2</vt:i4>", "<vt:i4>2</vt:i4><vt:i4>2</vt:i4>"),
    ];
    for b in bad {
        let input = with_app(&base, &b);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.add_sheet("C", 2).unwrap();
        assert_eq!(app(&input), app(&wb.to_bytes().unwrap()), "{b}");
    }
    // the good shape is updated, sizes included
    let mut wb = Workbook::from_bytes(with_app(&base, &good)).unwrap();
    wb.add_sheet("C", 2).unwrap();
    let a = String::from_utf8(app(&wb.to_bytes().unwrap()).unwrap()).unwrap();
    assert!(a.contains(r#"<vt:vector size="3" baseType="lpstr">"#), "{a}");
}
