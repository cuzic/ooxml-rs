//! xlsx Phase2 part 2 (ADR-0006): rich strings (§4.2), format read-back
//! (§3 "Reading") and the `dxfs` registry (§5.8).

mod common;

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use common::*;
use ooxml_core::{QName, RawNode, XmlDocument};
use ooxml_xlsx::{Color, DxfFormat, Error, Fill, Font, Format, FormatRead, SML_NS, Workbook};

fn base() -> Vec<u8> {
    Workbook::new().unwrap().to_bytes().unwrap()
}

fn parse(bytes: &[u8]) -> XmlDocument {
    XmlDocument::parse(bytes).unwrap()
}

fn entries(doc: &XmlDocument, table: &str) -> Vec<String> {
    let t = doc.root().elements().find(|e| e.name().ns() == SML_NS && e.name().local() == table);
    t.map(|n| n.elements().map(|e| src(doc, e).to_owned()).collect()).unwrap_or_default()
}

fn src<'s>(doc: &'s XmlDocument, n: &RawNode) -> &'s str {
    let s = n.span().unwrap();
    &doc.source()[s.start..s.end()]
}

fn sst(bytes: &[u8]) -> Vec<String> {
    let doc = parse(text(bytes, "xl/sharedStrings.xml").as_bytes());
    doc.root().elements().map(|e| src(&doc, e).to_owned()).collect()
}

fn dxfs(bytes: &[u8]) -> Vec<String> {
    entries(&parse(text(bytes, "xl/styles.xml").as_bytes()), "dxfs")
}

fn cell_value(bytes: &[u8], at: (u32, u32)) -> Option<Data> {
    let mut x: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec())).unwrap();
    let name = x.sheet_names()[0].clone();
    let r = x.worksheet_range(&name).unwrap();
    r.get_value(at).cloned()
}

fn bold() -> Font {
    Font { bold: true, ..Font::default() }
}

fn red_italic() -> Font {
    Font { italic: true, color: Some(Color::rgb(0xFF0000)), ..Font::default() }
}

// ------------------------------------------------------------ rich strings

#[test]
fn rich_strings_match_rust_xlsxwriter() {
    use rust_xlsxwriter as rx;
    let mut wb = Workbook::new().unwrap();
    wb.add_worksheet().unwrap().cell_mut(0, 0).unwrap().set_rich_string(&[(&Font::default(), "plain "), (&bold(), "bold"), (&red_italic(), " red")]).unwrap();
    let ours = wb.to_bytes().unwrap();
    let mut rwb = rx::Workbook::new();
    let rb = rx::Format::new().set_bold();
    let ri = rx::Format::new().set_italic().set_font_color(rx::Color::RGB(0xFF0000));
    rwb.add_worksheet().write_rich_string(0, 0, &[(&rx::Format::new(), "plain "), (&rb, "bold"), (&ri, " red")]).unwrap();
    let theirs = rwb.save_to_buffer().unwrap();
    assert_eq!(sst(&ours), sst(&theirs));
    assert_eq!(cell_value(&ours, (0, 0)), Some(Data::String("plain bold red".into())));
}

#[test]
fn rich_strings_are_reused_exactly_and_never_collide_with_plain() {
    let mut wb = Workbook::new().unwrap();
    {
        let mut ws = wb.add_worksheet().unwrap();
        ws.cell_mut(0, 0).unwrap().set_rich_string(&[(&bold(), "a"), (&red_italic(), "b")]).unwrap();
        ws.cell_mut(1, 0).unwrap().set_rich_string(&[(&bold(), "a"), (&red_italic(), "b")]).unwrap();
        ws.cell_mut(2, 0).unwrap().set_string("ab").unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let e = sst(&out);
    assert_eq!(e.len(), 2, "one rich si, one plain si: {e:?}");
    assert!(e[0].starts_with("<si><r>") && e[1] == "<si><t>ab</t></si>", "{e:?}");
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<c r="A1" t="s"><v>0</v></c>"#) && xml.contains(r#"<c r="A2" t="s"><v>0</v></c>"#), "{xml}");
    // across sessions: an identical rich si is reused
    let mut wb = Workbook::from_bytes(out.clone()).unwrap();
    wb.worksheet().unwrap().cell_mut(3, 0).unwrap().set_rich_string(&[(&bold(), "a"), (&red_italic(), "b")]).unwrap();
    let out2 = wb.to_bytes().unwrap();
    assert_eq!(sst(&out2), e);
    let root = parse(text(&out2, "xl/sharedStrings.xml").as_bytes());
    assert_eq!(root.root().attr(QName::new("", "count")), Some("4"));
    assert_eq!(root.root().attr(QName::new("", "uniqueCount")), Some("2"));
}

#[test]
fn rich_strings_fall_back_to_inline_runs() {
    let input = p1::book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(!wb.shared_strings_available().unwrap());
    wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_rich_string(&[(&bold(), " a"), (&Font::default(), "b")]).unwrap();
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    assert!(
        xml.contains(r#"<c r="A2" t="inlineStr"><is><r><rPr><b/><sz val="11"/><color theme="1"/><rFont val="Calibri"/><family val="2"/><scheme val="minor"/></rPr><t xml:space="preserve"> a</t></r><r><rPr><sz val="11"/><color theme="1"/><rFont val="Calibri"/><family val="2"/><scheme val="minor"/></rPr><t>b</t></r></is></c>"#),
        "a default-font run gets rPr unless it is the first: {xml}"
    );
    assert_eq!(cell_value(&out, (1, 0)), Some(Data::String(" ab".into())));
    let mut wb = Workbook::from_bytes(out).unwrap();
    assert_eq!(wb.worksheet().unwrap().cell(1, 0).unwrap().unwrap().value().unwrap(), ooxml_xlsx::CellValue::String(" ab".into()));
}

#[test]
fn rich_string_refusals_change_nothing() {
    let input = base();
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let bad_font = Font { name: String::new(), ..Font::default() };
        let long = "x".repeat(20_000);
        let cases: Vec<(&str, ooxml_xlsx::Result<()>)> = vec![
            ("no runs", ws.cell_mut(0, 0).unwrap().set_rich_string(&[])),
            ("empty run", ws.cell_mut(0, 0).unwrap().set_rich_string(&[(&bold(), "")])),
            ("bad font", ws.cell_mut(0, 0).unwrap().set_rich_string(&[(&bad_font, "x")])),
            ("too long", ws.cell_mut(0, 0).unwrap().set_rich_string(&[(&bold(), &long), (&bold(), &long)])),
        ];
        for (what, r) in cases {
            assert!(r.is_err(), "{what}");
        }
        assert!(matches!(ws.cell_mut(0, 0).unwrap().set_rich_string(&[(&bold(), &long), (&bold(), &long)]), Err(Error::StringTooLong(40_000))));
    }
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

// ------------------------------------------------------------ format read-back

fn matrix() -> Vec<Format> {
    vec![
        Format::new(),
        Format::new().set_bold(),
        Format::new().set_italic().set_font_size(14.0).set_font_name("Arial"),
        Format::new().set_font_color(Color::Theme { index: 4, tint: ooxml_xlsx::Tint::new(-0.25).unwrap() }),
        Format::new().set_background_color(Color::rgb(0xFFFF00)),
        Format::new().set_border("thin"),
        Format::new().set_num_format("0.00"),
        Format::new().set_num_format("0.000\" kg\""),
        Format::date(),
        Format::new().set_align("center").set_valign("top").set_text_wrap(),
        Format::new().set_unlocked().set_hidden(),
        Format::new().set_quote_prefix(),
        Format::new().set_num_format("0.000").set_bold().set_font_color(Color::rgb(0xFF0000)).set_background_color(Color::rgb(0xFFFF00)).set_border("thin"),
    ]
}

#[test]
fn format_read_back_rewrites_to_the_same_style() {
    let mut wb = Workbook::new().unwrap();
    let n = matrix().len() as u32;
    {
        let mut ws = wb.add_worksheet().unwrap();
        for (i, f) in matrix().iter().enumerate() {
            ws.cell_mut(i as u32, 0).unwrap().set_number_with_format(1.0, f).unwrap();
        }
    }
    let mut ws = wb.worksheet().unwrap();
    for i in 0..n {
        let s = ws.cell(i, 0).unwrap().unwrap().style_index().unwrap();
        let read = ws.format(i, 0).unwrap().unwrap();
        let FormatRead::Expressible(g) = read else { panic!("row {i}: {read:?}") };
        // the projection, written again, resolves to the very same xf
        ws.cell_mut(i, 1).unwrap().set_number_with_format(1.0, &g).unwrap();
        assert_eq!(ws.cell(i, 1).unwrap().unwrap().style_index().unwrap(), s, "row {i}: {g:?}");
    }
    assert_eq!(ws.format(100, 0).unwrap(), None, "missing cell");
}

#[test]
fn format_read_back_reports_what_is_not_expressible() {
    let s = text(&base(), "xl/styles.xml")
        .replacen(r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#, r#"<cellStyleXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#, 1)
        .replacen(
            r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
            r#"<cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" pivotButton="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment horizontal="left" relativeIndent="1"/></xf></cellXfs>"#,
            1,
        );
    let mut wb = Workbook::from_bytes(p1::with_part(&base(), "xl/styles.xml", &s)).unwrap();
    for (i, why) in [(1, "xfId 1"), (2, "pivotButton"), (3, "relativeIndent")] {
        match wb.format_of_style(i).unwrap() {
            FormatRead::NotExpressible { style_index, reasons } => {
                assert_eq!(style_index, i);
                assert!(reasons.iter().any(|r| r == why), "{i}: {reasons:?}");
            }
            r => panic!("{i}: {r:?}"),
        }
    }
    assert!(matches!(wb.format_of_style(9).unwrap(), FormatRead::NotExpressible { .. }), "out of range");
    // no styles part: the application default
    let mut wb = Workbook::from_bytes(p1::book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)], "", "")).unwrap();
    assert_eq!(wb.worksheet().unwrap().format(0, 0).unwrap(), Some(FormatRead::Expressible(Box::new(Format::new()))));
}

// ------------------------------------------------------------ dxfs

#[test]
fn dxfs_are_appended_in_canonical_form_and_reused() {
    let red_on_yellow = DxfFormat::new().set_bold().set_font_color(Color::rgb(0xFF0000)).set_background_color(Color::rgb(0xFFFF00));
    let input = base();
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert_eq!(wb.add_differential_format(&red_on_yellow).unwrap(), 0);
    assert_eq!(wb.add_differential_format(&red_on_yellow).unwrap(), 0, "reused within the session");
    assert_eq!(wb.add_differential_format(&DxfFormat::new().set_num_format("0.0%")).unwrap(), 1);
    let out = wb.to_bytes().unwrap();
    assert_l1_except(&input, &out, &["xl/styles.xml"]);
    let st = text(&out, "xl/styles.xml");
    assert!(
        st.contains(r#"<dxfs count="2"><dxf><font><b/><color rgb="FFFF0000"/></font><fill><patternFill><bgColor rgb="FFFFFF00"/></patternFill></fill></dxf><dxf><numFmt numFmtId="164" formatCode="0.0%"/></dxf></dxfs>"#),
        "{st}"
    );
    assert!(!st.contains("<numFmts"), "a dxf numFmt is self-contained: {st}");
    // across sessions: exact-form reuse; a cell format with the same code
    // later gets a different id (164 is taken in the package)
    let mut wb = Workbook::from_bytes(out.clone()).unwrap();
    assert_eq!(wb.add_differential_format(&red_on_yellow).unwrap(), 0);
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("0.0%")).unwrap();
    let out2 = wb.to_bytes().unwrap();
    assert_eq!(dxfs(&out2), dxfs(&out));
    assert!(text(&out2, "xl/styles.xml").contains(r#"<numFmt numFmtId="165" formatCode="0.0%"/>"#));
}

#[test]
fn dxfs_table_is_inserted_when_absent() {
    let s = text(&base(), "xl/styles.xml").replacen(r#"<dxfs count="0"/>"#, "", 1);
    let input = p1::with_part(&base(), "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.add_differential_format(&DxfFormat::new().set_italic()).unwrap();
    let st = text(&wb.to_bytes().unwrap(), "xl/styles.xml");
    assert!(st.contains(r#"</cellStyles><dxfs count="1"><dxf><font><i/></font></dxf></dxfs><tableStyles"#), "schema order: {st}");
}

#[test]
fn dxf_refusals_change_nothing() {
    // a conditional format already refers to dxf 2 of an empty table
    let input = p1::with_part(
        &base(),
        "xl/worksheets/sheet1.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><conditionalFormatting sqref="A1"><cfRule type="expression" dxfId="2" priority="1"><formula>TRUE</formula></cfRule></conditionalFormatting></worksheet>"#,
    );
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = wb.add_differential_format(&DxfFormat::new().set_bold());
    assert!(matches!(e, Err(Error::DanglingReference { ref table, index: 2 }) if table == "dxfs"), "{e:?}");
    let e = wb.add_differential_format(&DxfFormat::new().set_border("hairline"));
    assert!(matches!(e, Err(Error::InvalidFormat(_))), "{e:?}");
    let e = wb.add_differential_format(&DxfFormat { num_format: Some(ooxml_xlsx::NumFormat::Builtin(5)), ..DxfFormat::new() });
    assert!(matches!(e, Err(Error::InvalidFormat(_))), "built-in 5 has no fixed code: {e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    // an AC child in dxfs
    let s = text(&base(), "xl/styles.xml").replacen(
        r#"<dxfs count="0"/>"#,
        r#"<dxfs count="1"><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x14"><dxf/></mc:Choice><mc:Fallback><dxf/></mc:Fallback></mc:AlternateContent></dxfs>"#,
        1,
    );
    let input = p1::with_part(&base(), "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = wb.add_differential_format(&DxfFormat::new().set_bold());
    assert!(matches!(e, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

// ------------------------------------------------------------ sweep

/// One package: a rich string at a new row, every existing cell's format
/// read back (no panic, no error), and one dxf added; existing sst and dxf
/// entries keep their bytes, every other part is L1.
fn sweep_one(name: &str, input: &[u8], tally: &mut BTreeMap<&'static str, usize>) {
    let Ok(mut wb) = Workbook::from_bytes(input.to_vec()) else {
        *tally.entry("does not open").or_default() += 1;
        return;
    };
    if wb.edit_refusal().is_some() {
        *tally.entry("edits refused").or_default() += 1;
        return;
    }
    // read every cell's format first (a read: no part may change)
    if wb.worksheet().is_err() {
        *tally.entry("no worksheet").or_default() += 1;
        return;
    }
    let cells: Vec<(u32, u32)> = {
        let xml = text(input, wb.worksheet_part());
        let mut v = Vec::new();
        let mut rest = xml.as_str();
        while let Some(i) = rest.find(r#"<c r=""#) {
            rest = &rest[i + 6..];
            let r = &rest[..rest.find('"').unwrap()];
            if let Ok(c) = ooxml_xlsx::CellIndex::parse(r) {
                v.push((c.row, c.col));
            }
        }
        v
    };
    {
        let mut ws = wb.worksheet().unwrap();
        for &(r, c) in cells.iter().take(2_000) {
            match ws.format(r, c) {
                Ok(Some(FormatRead::Expressible(_))) => *tally.entry("expressible").or_default() += 1,
                Ok(Some(FormatRead::NotExpressible { .. })) => *tally.entry("not expressible").or_default() += 1,
                Ok(None) => {}
                Err(_) => *tally.entry("read refused").or_default() += 1,
            }
        }
    }
    let before = wb.to_bytes().unwrap();
    assert_l1_except(input, &before, &[]);
    let row = cells.iter().map(|c| c.0).max().map_or(0, |m| m + 1).min(1_048_575);
    let r = wb.worksheet().unwrap().cell_mut(row, 0).and_then(|c| c.set_rich_string(&[(&bold(), "rich "), (&red_italic(), "text")]));
    if r.is_err() {
        assert_l1_except(input, &wb.to_bytes().unwrap(), &[]);
        *tally.entry("rich refused").or_default() += 1;
    } else {
        *tally.entry("rich ok").or_default() += 1;
    }
    let d = wb.add_differential_format(&DxfFormat::new().set_bold().set_background_color(Color::rgb(0xC6EFCE)));
    *tally.entry(if d.is_ok() { "dxf ok" } else { "dxf refused" }).or_default() += 1;
    let out = wb.to_bytes().unwrap();
    // existing entries keep their bytes (append-only)
    let has = |b: &[u8], p: &str| ooxml_core::Package::from_bytes(b.to_vec()).unwrap().contains(p);
    if has(input, "xl/sharedStrings.xml") && has(&out, "xl/sharedStrings.xml") {
        let (a, b) = (sst(input), sst(&out));
        assert!(b.len() >= a.len() && b[..a.len()] == a[..], "{name}: sst entries changed");
    }
    if has(input, "xl/styles.xml") && has(&out, "xl/styles.xml") && d.is_ok() {
        let (a, b) = (dxfs(input), dxfs(&out));
        assert!(b.len() >= a.len() && b[..a.len()] == a[..], "{name}: dxf entries changed");
    }
    let sheet_name = wb.worksheet().unwrap().name().to_owned();
    if r.is_ok()
        && let Some(Data::String(s)) = try_calamine_cells(&out).and_then(|mut m| m.remove(&sheet_name)).and_then(|m| m.get(&(row, 0)).cloned())
    {
        assert_eq!(s, "rich text", "{name}");
    }
}

#[test]
fn part2_sweep_over_every_fixture() {
    let mut tally = BTreeMap::new();
    for name in all_zip_fixtures() {
        sweep_one(&name, &read(&name), &mut tally);
    }
    eprintln!("{tally:#?}");
    assert!(tally.get("rich ok").copied().unwrap_or(0) >= 15 && tally.get("dxf ok").copied().unwrap_or(0) >= 15, "{tally:#?}");
}

#[test]
#[ignore = "needs OOXML_XLSX_CORPUS=<directory of .xlsx/.xlsm files>"]
fn part2_sweep_over_a_local_corpus() {
    let dir = std::env::var("OOXML_XLSX_CORPUS").expect("set OOXML_XLSX_CORPUS");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xlsx") || x.eq_ignore_ascii_case("xlsm")))
        .collect();
    files.sort();
    let mut tally = BTreeMap::new();
    let mut failed = Vec::new();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let input = std::fs::read(f).unwrap();
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sweep_one(&name, &input, &mut tally))) {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
            failed.push(format!("{name}: {}", msg.lines().next().unwrap_or_default()));
        }
    }
    std::panic::set_hook(hook);
    eprintln!("{} files; {tally:#?}", files.len());
    assert!(failed.is_empty(), "{} packages failed:\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn fill_variants_round_trip_through_format_read() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.add_worksheet().unwrap();
    let f = Format { fill: Some(Fill::Pattern { pattern: "darkGrid".into(), fg: Some(Color::Indexed(10)), bg: Some(Color::Auto) }), ..Format::new() };
    ws.cell_mut(0, 0).unwrap().set_number_with_format(1.0, &f).unwrap();
    let Some(FormatRead::Expressible(g)) = ws.format(0, 0).unwrap() else { panic!() };
    assert_eq!(g.fill, f.fill);
}

#[test]
fn dxf_references_under_other_attribute_names_count_as_dangling() {
    // tables refer to dxfs as headerRowDxfId, dataDxfId, …
    let input = p1::with_part(
        &base(),
        "xl/worksheets/sheet1.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><extLst><ext uri="u"><x headerRowDxfId = '0'/></ext></extLst></worksheet>"#,
    );
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = wb.add_differential_format(&DxfFormat::new().set_bold());
    assert!(matches!(e, Err(Error::DanglingReference { ref table, index: 0 }) if table == "dxfs"), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

#[test]
fn border_colours_are_read_back() {
    use ooxml_xlsx::{Border, Side};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.add_worksheet().unwrap();
    let b = Border { bottom: Some(Side { style: "medium".into(), color: Some(Color::rgb(0x0000FF)) }), ..Border::default() };
    ws.cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format { border: Some(b.clone()), ..Format::new() }).unwrap();
    let Some(FormatRead::Expressible(g)) = ws.format(0, 0).unwrap() else { panic!() };
    assert_eq!(g.border, Some(b));
}

#[test]
fn dxf_number_formats_get_their_own_ids_and_never_leak_into_num_fmts() {
    let mut wb = Workbook::from_bytes(base()).unwrap();
    wb.add_differential_format(&DxfFormat::new().set_num_format("0.0%")).unwrap();
    wb.add_differential_format(&DxfFormat::new().set_num_format("0.00%%")).unwrap();
    // same session: a cell format with the dxf's code needs a numFmts entry
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("0.0%")).unwrap();
    let st = text(&wb.to_bytes().unwrap(), "xl/styles.xml");
    assert!(st.contains(r#"<dxf><numFmt numFmtId="164" formatCode="0.0%"/></dxf><dxf><numFmt numFmtId="165" formatCode="0.00%%"/></dxf>"#), "{st}");
    assert!(st.contains(r#"<numFmts count="1"><numFmt numFmtId="166" formatCode="0.0%"/></numFmts>"#), "{st}");
}

#[test]
fn dxf_borders_write_only_the_overriding_sides() {
    use ooxml_xlsx::{Border, Side};
    let mut wb = Workbook::from_bytes(base()).unwrap();
    let b = Border { bottom: Some(Side { style: "thin".into(), color: None }), ..Border::default() };
    wb.add_differential_format(&DxfFormat { border: Some(b), ..DxfFormat::new() }).unwrap();
    let st = text(&wb.to_bytes().unwrap(), "xl/styles.xml");
    assert!(st.contains(r#"<dxf><border><bottom style="thin"><color auto="1"/></bottom></border></dxf>"#), "{st}");
}
