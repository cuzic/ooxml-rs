//! Hand-built fixtures (ported from the spike): the oracle workbook, the
//! group workbook and the synthetic large sheet.

use std::io::{Cursor, Write};

const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

/// `styles.xml`'s content: `cellXfs` 0 default, 1 bold, 2 gray fill, 3 numFmt 14;
/// with `duplicate`, a fifth entry equal to the second.
pub(crate) fn custom_styles(duplicate: bool) -> String {
    let (count, extra) = if duplicate {
        (
            5,
            r#"<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#,
        )
    } else {
        (4, "")
    };
    format!(
        r#"<fonts count="2"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font><font><b/><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="{count}"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="0" fillId="1" borderId="0" xfId="0" applyFill="1"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>{extra}</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="0"/>"#
    )
}

/// A plain shared-string item.
pub(crate) fn plain(s: &str) -> String {
    format!("<si><t>{s}</t></si>")
}

/// A rich item: `"ab"` in a bold run, then `"c"`.
pub(crate) fn rich() -> String {
    r#"<si><r><rPr><b/><sz val="11"/><color theme="1"/><rFont val="Calibri"/><family val="2"/><scheme val="minor"/></rPr><t>ab</t></r><r><t>c</t></r></si>"#.to_owned()
}

fn zip(parts: &[(&str, String)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in parts {
        z.start_file(*name, opts).expect("zip");
        z.write_all(body.as_bytes()).expect("zip");
    }
    z.finish().expect("zip").into_inner()
}

/// A package of worksheets (`sheetId` = 1, 2, …), `sst` items (raw `si`
/// elements), an optional calcChain and a styles body.
pub(crate) fn package(
    sheets: &[(&str, String)],
    sst: &[String],
    calc_chain: Option<&str>,
    styles: &str,
) -> Vec<u8> {
    let decl = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
    let mut ct = String::from(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
    );
    let mut rels = String::from(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdT" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#,
    );
    let mut list = String::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, (name, body)) in sheets.iter().enumerate() {
        let n = i + 1;
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        list.push_str(&format!(
            r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#
        ));
        parts.push((
            format!("xl/worksheets/sheet{n}.xml"),
            format!("{decl}<worksheet {NS}>{body}</worksheet>"),
        ));
    }
    if let Some(cc) = calc_chain {
        ct.push_str(r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/>"#);
        rels.push_str(r#"<Relationship Id="rIdC" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>"#);
        parts.push(("xl/calcChain.xml".into(), format!(r#"{decl}<calcChain xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{cc}</calcChain>"#)));
    }
    ct.push_str("</Types>");
    rels.push_str("</Relationships>");
    let si: String = sst.concat();
    let mut all: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", format!("{decl}{ct}")),
        (
            "_rels/.rels",
            format!(
                r#"{decl}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml",
            format!(
                r#"{decl}<workbook {NS}><bookViews><workbookView/></bookViews><sheets>{list}</sheets><calcPr calcId="191029"/></workbook>"#
            ),
        ),
        ("xl/_rels/workbook.xml.rels", format!("{decl}{rels}")),
        (
            "xl/styles.xml",
            format!(
                r#"{decl}<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{styles}</styleSheet>"#
            ),
        ),
        (
            "xl/sharedStrings.xml",
            format!(
                r#"{decl}<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{}" uniqueCount="{}">{si}</sst>"#,
                sst.len(),
                sst.len()
            ),
        ),
    ];
    for (n, b) in &parts {
        all.push((n.as_str(), b.clone()));
    }
    zip(&all)
}

/// The oracle workbook. `Sheet1` (dimension `dim`):
///
/// | | A | B (`col/@style` = 1, bold) | C | D |
/// |---|---|---|---|---|
/// | 1 | 1 | "x" | `=A1*2` (2) | rich "ab" (bold) + "c" (`t="s"`, `si` 2) |
/// | 2 | 2 | | `=SUM(A1:A3)` (6) | inline "il" |
/// | 3 | 3 (`s=1` bold) | | 7 | |
/// | 4 | TRUE | "m" (merge `B4:C4`) | | |
/// | 5 | `=1+1` (2) | | | |
/// | 6 | row `customFormat`, `s=2` (gray fill), no cells | | | |
///
/// `Sheet2!A1` = `Sheet1!A1+1` (2). `cellXfs`: 0 default, 1 bold, 2 gray
/// fill, 3 numFmt 14. A calcChain lists the four formulas.
pub(crate) fn oracle_workbook(dim: &str) -> Vec<u8> {
    let s1 = format!(
        r#"<dimension ref="{dim}"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="2" max="2" width="10" style="1" customWidth="1"/></cols><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1" t="s"><v>0</v></c><c r="C1"><f>A1*2</f><v>2</v></c><c r="D1" t="s"><v>2</v></c></row><row r="2"><c r="A2"><v>2</v></c><c r="C2"><f>SUM(A1:A3)</f><v>6</v></c><c r="D2" t="inlineStr"><is><t>il</t></is></c></row><row r="3"><c r="A3" s="1"><v>3</v></c><c r="C3"><v>7</v></c></row><row r="4"><c r="A4" t="b"><v>1</v></c><c r="B4" t="s"><v>1</v></c></row><row r="5"><c r="A5"><f>1+1</f><v>2</v></c></row><row r="6" s="2" customFormat="1"/></sheetData><mergeCells count="1"><mergeCell ref="B4:C4"/></mergeCells>"#
    );
    let s2 = r#"<dimension ref="A1"/><sheetData><row r="1"><c r="A1"><f>Sheet1!A1+1</f><v>2</v></c></row></sheetData>"#.to_owned();
    package(
        &[("Sheet1", s1), ("Sheet2", s2)],
        &[plain("x"), plain("m"), rich()],
        Some(r#"<c r="C1" i="1"/><c r="C2"/><c r="A5"/><c r="A1" i="2"/>"#),
        &custom_styles(false),
    )
}

/// A workbook whose `Sheet2` depends on `Sheet1!A1` but has its rows out of
/// order: `edit_refusal()` is `None`, and the strip of that dependent's cache
/// is refused (`OutOfOrder`), so a write to `Sheet1!A1` is refused under the
/// default policy (PR 159 review, M1).
pub(crate) fn out_of_order_dependent_workbook() -> Vec<u8> {
    let s1 =
        r#"<dimension ref="A1"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#
            .to_owned();
    let s2 = r#"<dimension ref="A1:A2"/><sheetData><row r="2"><c r="A2"><v>3</v></c></row><row r="1"><c r="A1"><f>Sheet1!A1+1</f><v>2</v></c></row></sheetData>"#.to_owned();
    package(
        &[("Sheet1", s1), ("Sheet2", s2)],
        &[plain("x")],
        None,
        &custom_styles(false),
    )
}

/// A synthetic sheet of `rows` x 10 numbers (A..J) plus `K = A*2` formulas
/// with cached values, and a shared-string table.
pub(crate) fn large_workbook(rows: u32) -> Vec<u8> {
    let mut sd = String::with_capacity(rows as usize * 300);
    for r in 1..=rows {
        sd.push_str(&format!(r#"<row r="{r}" spans="1:11">"#));
        for (i, c) in "ABCDEFGHIJ".chars().enumerate() {
            sd.push_str(&format!(
                r#"<c r="{c}{r}"><v>{}</v></c>"#,
                r as usize * 10 + i
            ));
        }
        sd.push_str(&format!(
            r#"<c r="K{r}"><f>A{r}*2</f><v>{}</v></c></row>"#,
            r * 20
        ));
    }
    let s1 = format!(
        r#"<dimension ref="A1:K{rows}"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><sheetData>{sd}</sheetData>"#
    );
    package(
        &[("Sheet1", s1)],
        &[plain("x")],
        None,
        &custom_styles(false),
    )
}

/// The group workbook. `Sheet1` (dimension `dim`):
///
/// | | A | B | C | D | E | G | H |
/// |---|---|---|---|---|---|---|---|
/// | 1 | | | 10 | 20 | 30 | | `=SUM(C2:E3)` (129) |
/// | 2 | 1 | | member | **master** `$A2+D$1` (`ref` C2:E3) | member | `{=A2:A3*2}` (2) | |
/// | 3 | 2 | | member | member | member | (4) | |
/// | 4 | | `=D3+1` (23) | | | | | |
/// | 6 | 5 | 5 (merge `A6:B6`, a value in the non-anchor cell) | | | | | |
///
/// Shared group `si=0` over `C2:E3` with its master at `D2`. `Sheet2!A1` =
/// `Sheet1!C2*2` (22). A calcChain lists the formulas.
pub(crate) fn group_workbook(dim: &str) -> Vec<u8> {
    let m = |r: &str, v: u32| format!(r#"<c r="{r}"><f t="shared" si="0"/><v>{v}</v></c>"#);
    let s1 = format!(
        r#"<dimension ref="{dim}"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><sheetData><row r="1"><c r="C1"><v>10</v></c><c r="D1"><v>20</v></c><c r="E1"><v>30</v></c><c r="H1"><f>SUM(C2:E3)</f><v>129</v></c></row><row r="2"><c r="A2"><v>1</v></c>{}<c r="D2"><f t="shared" ref="C2:E3" si="0">$A2+D$1</f><v>21</v></c>{}<c r="G2"><f t="array" ref="G2:G3">A2:A3*2</f><v>2</v></c></row><row r="3"><c r="A3"><v>2</v></c>{}{}{}<c r="G3"><v>4</v></c></row><row r="4"><c r="B4"><f>D3+1</f><v>23</v></c></row><row r="6"><c r="A6"><v>5</v></c><c r="B6"><v>5</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A6:B6"/></mergeCells>"#,
        m("C2", 11),
        m("E2", 31),
        m("C3", 12),
        m("D3", 22),
        m("E3", 32),
    );
    let s2 = r#"<dimension ref="A1"/><sheetData><row r="1"><c r="A1"><f>Sheet1!C2*2</f><v>22</v></c></row></sheetData>"#.to_owned();
    package(
        &[("Sheet1", s1), ("Sheet2", s2)],
        &[plain("x")],
        Some(
            r#"<c r="H1" i="1"/><c r="D2"/><c r="C2"/><c r="E2"/><c r="C3"/><c r="D3"/><c r="E3"/><c r="G2"/><c r="B4"/><c r="A1" i="2"/>"#,
        ),
        &custom_styles(false),
    )
}

#[test]
fn every_fixture_is_editable() {
    use crate::workbook::Workbook;
    for (name, bytes) in [
        ("oracle A1:C5", oracle_workbook("A1:C5")),
        ("oracle A1:A2", oracle_workbook("A1:A2")),
        ("oracle A1", oracle_workbook("A1")),
        ("group", group_workbook("A1:H6")),
        ("large", large_workbook(20)),
    ] {
        let wb = Workbook::from_bytes(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            wb.edit_refusal().is_none(),
            "{name}: {:?}",
            wb.edit_refusal()
        );
    }
}
