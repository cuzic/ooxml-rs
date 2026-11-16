//! PR #50 review round 1: regression tests for M1–M4 and m1–m8 (the
//! reviewer's probes P1–P8 and the mutants that survived: R01, R07, R11,
//! R12, R13, R14, R15, R17, R18, R20, R22).

mod common;

use common::*;
use ooxml_xlsx::{Color, Error, ExcelDateTime, Fill, Font, Format, Tint, Workbook};

fn base() -> Vec<u8> {
    Workbook::new().unwrap().to_bytes().unwrap()
}

fn styles(b: &[u8]) -> String {
    text(b, "xl/styles.xml")
}

fn with_styles(s: &str) -> Vec<u8> {
    p1::with_part(&base(), "xl/styles.xml", s)
}

fn with_sheet(b: &[u8], body: &str) -> Vec<u8> {
    p1::with_part(b, "xl/worksheets/sheet1.xml", &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{body}</worksheet>"#))
}

fn table<'s>(s: &'s str, name: &str) -> &'s str {
    let a = s.find(&format!("<{name}")).unwrap_or_else(|| panic!("no {name} in {s}"));
    let end = format!("</{name}>");
    let b = s[a..].find(&end).map_or_else(|| s[a..].find("/>").unwrap() + 2, |i| i + end.len()) + a;
    &s[a..b]
}

fn write(input: &[u8], at: (u32, u32), f: &Format) -> ooxml_xlsx::Result<Vec<u8>> {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    wb.worksheet().unwrap().cell_mut(at.0, at.1).unwrap().set_number_with_format(1.0, f)?;
    Ok(wb.to_bytes().unwrap())
}

const FONT0: &str = r#"<font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>"#;
const XF0: &str = r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#;

// ------------------------------------------------------------ M1

#[test]
fn m1_datetime_values_are_always_valid() {
    // the fields are private: impossible dates cannot be built
    assert!(ExcelDateTime::from_ymd(2026, 2, 31).is_none());
    assert!(ExcelDateTime::from_ymd_hms(2026, 2, 1, 99, 0, 0.0).is_none());
    assert!(ExcelDateTime::from_ymd_hms(2026, 2, 1, 0, 0, f64::NAN).is_none());
    let mut wb = Workbook::from_bytes(base()).unwrap();
    let d = ExcelDateTime::from_ymd_hms(2026, 2, 28, 23, 59, 59.5).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_datetime(d).unwrap();
    let xml = sheet_xml(&wb.to_bytes().unwrap());
    assert!(!xml.contains("NaN") && xml.contains("<v>46081.99"), "{xml}");
    assert_eq!((d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second()), (Some(2026), 2, 28, 23, 59, 59.5));
}

// ------------------------------------------------------------ M2, m1, m2, m3

#[test]
fn m2_builtin_ids_from_164_are_refused() {
    for id in [164, 165, 999] {
        let e = write(&base(), (0, 0), &Format::new().set_num_format_index(id));
        assert!(matches!(e, Err(Error::InvalidFormat(_))), "{id}: {e:?}");
    }
    // the reviewer's P8: no later custom id gives A1's id a meaning
    let out = write(&base(), (0, 0), &Format::new().set_num_format_index(5)).unwrap();
    assert!(table(&styles(&out), "cellXfs").contains(r#"numFmtId="5""#));
}

#[test]
fn m1_locale_dependent_codes_are_not_mapped_to_14_or_22() {
    let out = write(&base(), (0, 0), &Format::new().set_num_format("mm-dd-yy")).unwrap();
    let s = styles(&out);
    assert!(table(&s, "numFmts").contains(r#"numFmtId="164" formatCode="mm-dd-yy""#), "{s}");
    let out = write(&base(), (0, 0), &Format::new().set_num_format("m/d/yy h:mm")).unwrap();
    assert!(styles(&out).contains(r#"formatCode="m/d/yy h:mm""#));
    // an explicit built-in 14 is still allowed
    let out = write(&base(), (0, 0), &Format::new().set_num_format_index(14)).unwrap();
    assert!(table(&styles(&out), "cellXfs").contains(r#"numFmtId="14""#));
}

#[test]
fn m2_general_is_refused_when_id_0_is_redefined_and_m3_date_falls_back() {
    let s = styles(&base()).replace("<fonts", r#"<numFmts count="2"><numFmt numFmtId="0" formatCode="@"/><numFmt numFmtId="14" formatCode="yyyy/m/d"/></numFmts><fonts"#);
    let input = with_styles(&s);
    let e = write(&input, (0, 0), &Format { num_format: Some(ooxml_xlsx::NumFormat::General), ..Format::new() });
    assert!(matches!(e, Err(Error::BuiltinNumFormatRedefined { id: 0 })), "{e:?}");
    let out = write(&input, (0, 0), &Format::date()).unwrap();
    assert!(styles(&out).contains(r#"formatCode="yyyy-mm-dd""#), "Format::date() falls back to yyyy-mm-dd when 14 is redefined");
}

// ------------------------------------------------------------ M3

#[test]
fn m3_japanese_default_font_is_kept() {
    let jp = r#"<font><sz val="11"/><color theme="1"/><name val="游ゴシック"/><family val="2"/><charset val="128"/><scheme val="minor"/></font>"#;
    let input = with_styles(&styles(&base()).replacen(FONT0, jp, 1));
    let mut wb = Workbook::from_bytes(input).unwrap();
    let f = wb.default_font().unwrap().expect("expressible with charset");
    assert_eq!((f.name.as_str(), f.charset), ("游ゴシック", Some(128)));
    let bold = Format::with_default_font_of(&mut wb).unwrap().set_bold();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &bold).unwrap();
    let out = wb.to_bytes().unwrap();
    let fonts = table(&styles(&out), "fonts").to_owned();
    assert!(fonts.ends_with(r#"<font><b/><sz val="11"/><color theme="1"/><name val="游ゴシック"/><family val="2"/><charset val="128"/><scheme val="minor"/></font></fonts>"#), "{fonts}");
    // the unchanged default font is reused byte for byte
    let mut wb = Workbook::from_bytes(out.clone()).unwrap();
    let same = Format::with_default_font_of(&mut wb).unwrap();
    wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_number_with_format(1.0, &same).unwrap();
    assert_eq!(table(&styles(&wb.to_bytes().unwrap()), "fonts"), fonts);
}

// ------------------------------------------------------------ M4

#[test]
fn m4_missing_cells_start_from_the_row_or_column_style() {
    let s = styles(&base())
        .replacen(r#"<fonts count="1">"#, r#"<fonts count="2">"#, 1)
        .replacen("</fonts>", r#"<font><b/><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font></fonts>"#, 1)
        .replacen(&format!(r#"<cellXfs count="1">{XF0}</cellXfs>"#), &format!(r#"<cellXfs count="2">{XF0}<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>"#), 1);
    let input = with_sheet(&with_styles(&s), r#"<cols><col min="2" max="2" width="9" style="1" customWidth="1"/></cols><sheetData><row r="1" s="1" customFormat="1"><c r="A1"><v>1</v></c></row><row r="7" s="1"><c r="A7"><v>1</v></c></row></sheetData>"#);
    let mut wb = Workbook::from_bytes(input).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(4, 1).unwrap().set_num_format("0.00").unwrap(); // B5: column style 1
        ws.cell_mut(0, 3).unwrap().set_num_format("0.0").unwrap(); // D1: row style 1 (customFormat)
        ws.cell_mut(6, 3).unwrap().set_num_format("0.000").unwrap(); // D7: row s without customFormat: xf 0
    }
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    let s_of = |r: &str| {
        let a = xml.find(&format!(r#"<c r="{r}""#)).unwrap();
        let t = &xml[a..a + xml[a..].find('>').unwrap()];
        t.split(r#"s=""#).nth(1).map(|v| v[..v.find('"').unwrap()].parse::<usize>().unwrap()).unwrap()
    };
    let st = styles(&out);
    let xfs: Vec<&str> = table(&st, "cellXfs").split("<xf ").skip(1).collect();
    assert!(xfs[s_of("B5")].contains(r#"fontId="1""#), "B5 keeps the column's bold: {}", xfs[s_of("B5")]);
    assert!(xfs[s_of("D1")].contains(r#"fontId="1""#), "D1 keeps the row's bold: {}", xfs[s_of("D1")]);
    assert!(xfs[s_of("D7")].contains(r#"fontId="0""#), "a row s without customFormat is not applied: {}", xfs[s_of("D7")]);
}

// ------------------------------------------------------------ m4

#[test]
fn m4_more_values_are_validated() {
    let long = "x".repeat(32);
    let cases = [
        Format::new().set_num_format(""),
        Format::new().set_font_color(Color::Theme { index: 12, tint: Tint::NONE }),
        Format::new().set_font_name(&long),
        Format { font: Some(Font { family: Some(15), ..Font::default() }), ..Format::new() },
        Format { alignment: Some(ooxml_xlsx::Alignment { indent: Some(251), ..Default::default() }), ..Format::new() },
        Format { fill: Some(Fill::Pattern { pattern: "solid".into(), fg: Some(Color::Theme { index: 40, tint: Tint::NONE }), bg: None }), ..Format::new() },
    ];
    for f in cases {
        let e = write(&base(), (0, 0), &f);
        assert!(matches!(e, Err(Error::InvalidFormat(_))), "{f:?}: {e:?}");
    }
    write(&base(), (0, 0), &Format::new().set_font_name(&"x".repeat(31))).unwrap();
}

// ------------------------------------------------------------ m5

#[test]
fn m5_is_date_is_a_read() {
    // no styles part: false, not NoStylesPart
    let input = p1::book(&[("S", r#"<row r="1"><c r="A1" s="1"><v>1</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(!wb.worksheet().unwrap().is_date(0, 0).unwrap());
    // with styles: no registry activation, so a part the numFmtId scan
    // cannot decode does not matter for reading
    let mut wb = Workbook::from_bytes(base()).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_datetime(ExcelDateTime::from_ymd(2026, 1, 1).unwrap()).unwrap();
    let out = wb.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(out.clone()).unwrap();
    assert!(wb.worksheet().unwrap().is_date(0, 0).unwrap());
    assert_l1_except(&out, &wb.to_bytes().unwrap(), &[]);
}

// ------------------------------------------------------------ m6, R14, R15

#[test]
fn m6_default_style_inside_alternate_content_is_refused() {
    let csx = r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#;
    let ac = r#"<cellStyleXfs count="1"><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x14"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></mc:Choice><mc:Fallback><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></mc:Fallback></mc:AlternateContent></cellStyleXfs>"#;
    let input = with_styles(&styles(&base()).replacen(csx, ac, 1));
    let e = write(&input, (0, 0), &Format::new().set_bold());
    assert!(matches!(e, Err(Error::NotExpressible(_))), "{e:?}");
}

#[test]
fn r14_r15_unset_groups_inherit_cell_style_xfs_0_not_cell_xfs_0() {
    let csx = r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#;
    let s = styles(&base())
        .replacen(r#"<fonts count="1">"#, r#"<fonts count="2">"#, 1)
        .replacen("</fonts>", r#"<font><sz val="10"/><name val="Arial"/><family val="2"/></font></fonts>"#, 1)
        .replacen(csx, r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="1" fillId="0" borderId="0"><alignment vertical="center"/></xf></cellStyleXfs>"#, 1);
    let out = write(&with_styles(&s), (0, 0), &Format::new().set_num_format("0.0")).unwrap();
    let st = styles(&out);
    assert!(table(&st, "cellXfs").ends_with(r#"<xf numFmtId="164" fontId="1" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"><alignment vertical="center"/></xf></cellXfs>"#), "{st}");
}

// ------------------------------------------------------------ m7, R11

#[test]
fn m7_r11_numfmtid_scan_decodes_references_and_utf16() {
    let mut pkg = ooxml_core::Package::from_bytes(base()).unwrap();
    pkg.set_part("xl/worksheets/sheet1.xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><extLst><ext uri="u"><x numFmtId="&#49;70"/></ext></extLst></worksheet>"#.to_vec()).unwrap();
    let out = write(&pkg.to_bytes().unwrap(), (0, 0), &Format::new().set_num_format("0.0")).unwrap();
    assert!(styles(&out).contains(r#"numFmtId="171""#), "{}", styles(&out));
    // a UTF-16 part (here the theme, which the workbook never parses) is
    // decoded, not skipped
    let utf16 = r#"<?xml version="1.0" encoding="UTF-16"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><x numFmtId="300"/></a:theme>"#;
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(utf16.encode_utf16().flat_map(u16::to_le_bytes));
    let mut pkg = ooxml_core::Package::from_bytes(base()).unwrap();
    pkg.set_part("xl/theme/theme1.xml", bytes).unwrap();
    let out = write(&pkg.to_bytes().unwrap(), (0, 0), &Format::new().set_num_format("0.0")).unwrap();
    assert!(styles(&out).contains(r#"numFmtId="301""#), "{}", styles(&out));
}

// ------------------------------------------------------------ R01, R22: limits

#[test]
fn r01_a_table_may_reach_its_limit() {
    let fonts: String = (0..511).map(|i| FONT0.replace(r#"val="11""#, &format!(r#"val="{}""#, 8 + i % 300))).collect();
    let s = styles(&base()).replacen(&format!(r#"<fonts count="1">{FONT0}</fonts>"#), &format!(r#"<fonts count="511">{fonts}</fonts>"#), 1);
    let out = write(&with_styles(&s), (0, 0), &Format::new().set_font_name("Arial")).expect("the 512th font is allowed");
    assert!(table(&styles(&out), "fonts").starts_with(r#"<fonts count="512">"#));
}

#[test]
fn r22_the_cell_format_limit_counts_cell_style_xfs() {
    let csx = r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#;
    let many: String = (0..65_489).map(|_| r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>"#).collect();
    let s = styles(&base()).replacen(csx, &format!(r#"<cellStyleXfs count="65489">{many}</cellStyleXfs>"#), 1);
    // cellXfs 1 + cellStyleXfs 65,489 = 65,490: one more xf is too many
    let e = write(&with_styles(&s), (0, 0), &Format::new().set_bold());
    assert!(matches!(e, Err(Error::StyleTableFull { ref table, .. }) if table == "cellXfs+cellStyleXfs"), "{e:?}");
}

// ------------------------------------------------------------ R07, R12, R13

#[test]
fn r07_alternate_content_without_fallback_is_not_index_stable() {
    let ac = format!(r#"<cellXfs count="2">{XF0}<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x14">{XF0}</mc:Choice></mc:AlternateContent></cellXfs>"#);
    let input = with_styles(&styles(&base()).replacen(&format!(r#"<cellXfs count="1">{XF0}</cellXfs>"#), &ac, 1));
    let mut wb = Workbook::from_bytes(input).unwrap();
    let e = wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_style_index(0);
    assert!(matches!(e, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))), "{e:?}");
}

#[test]
fn r12_cell_style_xfs_references_count_as_dangling() {
    let csx = r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#;
    let s = styles(&base()).replacen(csx, r#"<cellStyleXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0"/></cellStyleXfs>"#, 1);
    let e = write(&with_styles(&s), (0, 0), &Format::new().set_bold());
    assert!(matches!(e, Err(Error::DanglingReference { ref table, index: 1 }) if table == "fonts"), "{e:?}");
}

#[test]
fn r13_fills_0_is_reused_only_as_the_canonical_none_fill() {
    // fills[0] holds exactly the canonical yellow fill: still not a reuse
    // target for yellow (index 0 means "no fill" to readers; ADR-0006 n4)
    let yellow = r#"<fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/><bgColor indexed="64"/></patternFill></fill>"#;
    let s = styles(&base()).replacen(r#"<fill><patternFill patternType="none"/></fill>"#, yellow, 1);
    let out = write(&with_styles(&s), (0, 0), &Format::new().set_background_color(Color::rgb(0xFFFF00))).unwrap();
    let st = styles(&out);
    assert!(table(&st, "fills").starts_with(r#"<fills count="3">"#), "{st}");
    assert!(table(&st, "cellXfs").contains(r#"fillId="2""#), "{st}");
}

// ------------------------------------------------------------ R17, R18: absent counts stay absent

#[test]
fn r17_r18_absent_count_attributes_stay_absent() {
    let input = p1::with_part(&base(), "xl/sharedStrings.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" uniqueCount="0"/>"#);
    let s = styles(&input).replacen(r#"<fonts count="1">"#, "<fonts>", 1);
    let input = p1::with_part(&input, "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(input).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(0, 0).unwrap().set_string("x").unwrap();
        ws.cell_mut(0, 1).unwrap().set_number_with_format(1.0, &Format::new().set_bold()).unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let sst = text(&out, "xl/sharedStrings.xml");
    assert!(sst.contains(r#"uniqueCount="1""#) && !sst.contains(" count="), "{sst}");
    assert!(table(&styles(&out), "fonts").starts_with("<fonts><font>"), "{}", styles(&out));
}

// ------------------------------------------------------------ R20

#[test]
fn r20_date1904_true_is_accepted() {
    let wbx = text(&base(), "xl/workbook.xml").replacen("<workbookPr ", r#"<workbookPr date1904="true" "#, 1);
    let mut wb = Workbook::from_bytes(p1::with_part(&base(), "xl/workbook.xml", &wbx)).unwrap();
    assert!(wb.date1904().unwrap());
}
