//! Regression tests for the PR #53 review, round 1
//! (docs/reviews/pr53-round1.md): each finding as an observable outcome.

mod common;

use std::assert_matches;

use common::*;
use ooxml_xlsx::{Error, Workbook};

const WS_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const PIVOT_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
const CHART_CT: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const TABLE_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";

fn ws(inner: &str) -> Vec<u8> {
    format!(r#"<worksheet xmlns="{WS_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">{inner}</worksheet>"#).into_bytes()
}

fn ct_add(input: &[u8], extra: &str) -> (&'static str, Vec<u8>) {
    ("[Content_Types].xml", text(input, "[Content_Types].xml").replace("</Types>", &format!("{extra}</Types>")).into_bytes())
}

fn override_(part: &str, ct: &str) -> String {
    format!(r#"<Override PartName="/{part}" ContentType="{ct}"/>"#)
}

/// Asserts a refusal that leaves the package byte-identical.
fn refuses(input: &[u8], op: impl FnOnce(&mut Workbook) -> ooxml_xlsx::Result<Vec<ooxml_xlsx::RewrittenLocation>>) -> Error {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let e = op(&mut wb).expect_err("must refuse");
    assert_unchanged(&wb, input);
    e
}

fn two_sheets() -> Vec<u8> {
    common::p1::book(&[("A", ""), ("Data", "")], "", "")
}

// ---- B1: every formula-bearing element is rewritten or refused ----

#[test]
fn x14_formulas_refuse() {
    let base = two_sheets();
    let dv = r#"<sheetData/><extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"><x14:dataValidations count="1"><x14:dataValidation type="list"><x14:formula1><xm:f>Data!$A$1:$A$3</xm:f></x14:formula1><xm:sqref>A1</xm:sqref></x14:dataValidation></x14:dataValidations></ext></extLst>"#;
    let sp = r#"<sheetData/><extLst><ext uri="{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"><x14:sparklineGroups><x14:sparklineGroup><x14:sparklines><x14:sparkline><xm:f>Data!A1:E1</xm:f><xm:sqref>F1</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext></extLst>"#;
    // an SML-namespace <f> outside sheetData/row/c (not what the rewriter edits)
    let stray = r#"<sheetData/><extLst><ext uri="{X}"><f>Data!A1</f></ext></extLst>"#;
    for inner in [dv, sp] {
        let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(inner))]);
        assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
        assert_matches!(refuses(&input, |wb| wb.delete_sheet("Data")), Error::UnrewritableReference { .. });
    }
    // the census already refuses a stray SML `f`
    let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(stray))]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::FormulaScanFailed { .. });
}

#[test]
fn c15_chart_formulas_are_rewritten() {
    let base = two_sheets();
    let chart = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:c15="http://schemas.microsoft.com/office/drawing/2012/chart"><c:chart><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:f>Data!$A$1</c:f></c:strRef></c:tx><c:extLst><c:ext uri="{02D57815-91ED-43cb-92C2-25804820EDAC}"><c15:datalabelsRange><c15:f>Data!$C$1:$C$3</c15:f></c15:datalabelsRange></c:ext></c:extLst></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
    let input = rebuild(&base, &[("xl/charts/chart1.xml", chart.as_bytes().to_vec()), ct_add(&base, &override_("xl/charts/chart1.xml", CHART_CT))]);
    let mut wb = Workbook::from_bytes(input).unwrap();
    let rw = wb.rename_sheet("Data", "New").unwrap();
    let c = text(&wb.to_bytes().unwrap(), "xl/charts/chart1.xml");
    assert!(c.contains("<c:f>New!$A$1</c:f>") && c.contains("<c15:f>New!$C$1:$C$3</c15:f>"), "{c}");
    assert_eq!(rw.iter().filter(|r| r.part == "xl/charts/chart1.xml").count(), 2);
}

#[test]
fn unparseable_chart_refuses() {
    let base = two_sheets();
    // quick-xml reads it, the tree parser refuses the DTD
    let chart = r#"<!DOCTYPE x [<!ENTITY e "z">]><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:f>Data!$A$1</c:f></c:chartSpace>"#;
    let input = rebuild(&base, &[("xl/charts/chart1.xml", chart.as_bytes().to_vec()), ct_add(&base, &override_("xl/charts/chart1.xml", CHART_CT))]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
}

// ---- B2 / M2: pivot cache sources ----

fn with_pivot(base: &[u8], pc: &str) -> Vec<u8> {
    let rels = text(base, "xl/_rels/workbook.xml.rels").replace(
        "</Relationships>",
        r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="pivotCache/pivotCacheDefinition1.xml"/></Relationships>"#,
    );
    rebuild(base, &[
        ("xl/pivotCache/pivotCacheDefinition1.xml", pc.as_bytes().to_vec()),
        ("xl/_rels/workbook.xml.rels", rels.into_bytes()),
        ct_add(base, &override_("xl/pivotCache/pivotCacheDefinition1.xml", PIVOT_CT)),
    ])
}

fn pivot(source: &str) -> String {
    format!(r#"<pivotCacheDefinition xmlns="{WS_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" refreshOnLoad="1" recordCount="0"><cacheSource type="worksheet">{source}</cacheSource><cacheFields count="0"/></pivotCacheDefinition>"#)
}

#[test]
fn pivot_source_sheet_follows_a_rename() {
    let input = with_pivot(&two_sheets(), &pivot(r#"<worksheetSource ref="A1:B3" sheet="data"/>"#));
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let rw = wb.rename_sheet("Data", "New").unwrap();
    let out = wb.to_bytes().unwrap();
    let pc = text(&out, "xl/pivotCache/pivotCacheDefinition1.xml");
    assert!(pc.contains(r#"<worksheetSource ref="A1:B3" sheet="New"/>"#), "{pc}");
    assert!(rw.iter().any(|r| r.location == "worksheetSource sheet" && r.before == "data" && r.after == "New"));
    // another workbook's source (r:id) is left alone
    let ext = with_pivot(&two_sheets(), &pivot(r#"<worksheetSource ref="A1:B3" sheet="Data" r:id="rId1"/>"#));
    let mut wb = Workbook::from_bytes(ext.clone()).unwrap();
    wb.rename_sheet("Data", "New").unwrap();
    assert_eq!(text(&wb.to_bytes().unwrap(), "xl/pivotCache/pivotCacheDefinition1.xml"), text(&ext, "xl/pivotCache/pivotCacheDefinition1.xml"));
}

#[test]
fn consolidation_sources_refuse_a_rename() {
    let src = r#"<consolidation><rangeSets count="1"><rangeSet ref="A1:B3" sheet="Data"/></rangeSets></consolidation>"#;
    let input = with_pivot(&two_sheets(), &pivot(src).replace(r#"type="worksheet""#, r#"type="consolidation""#));
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
}

#[test]
fn pivot_source_through_a_defined_name_refuses_delete() {
    for dn in [r#"<definedName name="Src">Data!$A$1:$B$3</definedName>"#, r#"<definedName name="Src" localSheetId="0">'Data'!$A$1:$B$3</definedName>"#] {
        let base = common::p1::book(&[("A", ""), ("Data", "")], dn, "");
        let input = with_pivot(&base, &pivot(r#"<worksheetSource name="src"/>"#));
        assert_matches!(refuses(&input, |wb| wb.delete_sheet("Data")), Error::SheetIsPivotSource(_));
    }
    // a name on another sheet does not
    let base = common::p1::book(&[("A", ""), ("Data", "")], r#"<definedName name="Src">A!$A$1:$B$3</definedName>"#, "");
    let input = with_pivot(&base, &pivot(r#"<worksheetSource name="Src"/>"#));
    Workbook::from_bytes(input).unwrap().delete_sheet("Data").unwrap();
}

// ---- m2: strict delete precision ----

#[test]
fn strict_delete_counts_what_lenient_leaves() {
    let base = common::p1::book(&[("A", "<row r=\"1\"><c r=\"A1\"><f>SUM(B:D!A1)</f><v>3</v></c></row>"), ("B", ""), ("C", ""), ("D", "")], "", "");
    assert_matches!(refuses(&base, |wb| wb.delete_sheet_strict("C")), Error::SheetInUse { count: 1, .. });
    let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(r#"<sheetData/><hyperlinks><hyperlink ref="A2" location="C!A1"/></hyperlinks>"#))]);
    assert_matches!(refuses(&input, |wb| wb.delete_sheet_strict("C")), Error::SheetInUse { count: 1, .. });
}

#[test]
fn own_local_names_are_not_rewrites() {
    let base = common::p1::book(&[("A", ""), ("B", "")], r#"<definedName name="_xlnm.Print_Area" localSheetId="1">B!$A$1:$C$10</definedName>"#, "");
    let mut wb = Workbook::from_bytes(base.clone()).unwrap();
    assert_eq!(wb.delete_sheet("B").unwrap(), vec![]);
    let mut wb = Workbook::from_bytes(base).unwrap();
    assert_eq!(wb.delete_sheet_strict("B").unwrap(), vec![]);
}

// ---- m3: table-in-use precision ----

fn table_book(sheet_b: &str, extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let base = common::p1::book(&[("A", ""), ("B", "")], "", "");
    let t = format!(r#"<table xmlns="{WS_NS}" id="7" name="T1" displayName="T1" ref="A1:A3"><tableColumns count="1"><tableColumn id="1" name="x"/></tableColumns></table>"#);
    let mut edits = vec![
        ("xl/worksheets/sheet2.xml", ws(&format!(r#"<sheetData>{sheet_b}</sheetData><tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#))),
        (
            "xl/worksheets/_rels/sheet2.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#.to_vec(),
        ),
        ("xl/tables/table1.xml", t.into_bytes()),
    ];
    let mut ct = override_("xl/tables/table1.xml", TABLE_CT);
    for (p, b) in extra {
        edits.push((p, b.clone()));
        if p.ends_with(".xml") && !p.contains("worksheets") {
            ct.push_str(&override_(p, "application/vnd.ms-excel.slicerCache+xml"));
        }
    }
    edits.push(ct_add(&base, &ct));
    rebuild(&base, &edits)
}

#[test]
fn table_used_only_on_its_sheet_does_not_refuse() {
    let input = table_book(r#"<row r="5"><c r="A5"><f>SUM(T1[x])</f></c></row>"#, &[]);
    Workbook::from_bytes(input).unwrap().delete_sheet("B").unwrap();
}

#[test]
fn table_slicer_cache_refuses_delete() {
    let sc = br#"<slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" name="Slicer_x"><extLst><x:ext xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" uri="{2F2917AC-EB37-4324-AD4E-5DD8C200BD13}"><x15:tableSlicerCache xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main" tableId="7" column="1"/></x:ext></extLst></slicerCacheDefinition>"#;
    let input = table_book("", &[("xl/slicerCaches/slicerCache1.xml", sc.to_vec())]);
    assert_matches!(refuses(&input, |wb| wb.delete_sheet("B")), Error::TableInUse(_));
}

#[test]
fn table_in_data_validation_refuses_delete() {
    let base = table_book("", &[]);
    let dv = ws(r#"<sheetData/><dataValidations count="1"><dataValidation type="list" sqref="A1"><formula1>INDIRECT("T1[x]")</formula1></dataValidation></dataValidations>"#);
    let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", dv)]);
    assert_matches!(refuses(&input, |wb| wb.delete_sheet("B")), Error::TableInUse(_));
}

// ---- m5: firstSheet ----

#[test]
fn views_without_first_sheet_stay_without() {
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", "")], "", r#"<bookViews><workbookView activeTab="2"/></bookViews>"#);
    for op in [0, 1, 2] {
        let mut wb = Workbook::from_bytes(base.clone()).unwrap();
        match op {
            0 => wb.move_sheet(2, 0).unwrap(),
            1 => drop(wb.delete_sheet("A").unwrap()),
            _ => wb.add_sheet("N", 0).unwrap(),
        }
        let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
        assert!(!w.contains("firstSheet"), "op {op}: {w}");
    }
}

#[test]
fn move_keeps_first_sheet_clamped() {
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", ""), ("D", "")], "", r#"<bookViews><workbookView firstSheet="1" activeTab="3"/></bookViews>"#);
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.move_sheet(3, 0).unwrap(); // the active sheet moves to the front
    let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(w.contains(r#"firstSheet="0""#) && w.contains(r#"activeTab="0""#), "{w}");
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", ""), ("D", "")], "", r#"<bookViews><workbookView firstSheet="1" activeTab="3"/></bookViews>"#);
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.move_sheet(0, 2).unwrap(); // the tab strip stays where it was
    let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(w.contains(r#"firstSheet="1""#), "{w}");
}

/// Add and delete shift `firstSheet` with the positions (review mutant R1).
#[test]
fn add_and_delete_shift_first_sheet() {
    let views = r#"<bookViews><workbookView firstSheet="1" activeTab="2"/></bookViews>"#;
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", "")], "", views);
    let mut wb = Workbook::from_bytes(base.clone()).unwrap();
    wb.add_sheet("N", 0).unwrap();
    let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(w.contains(r#"firstSheet="2""#) && w.contains(r#"activeTab="3""#), "{w}");
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.delete_sheet("A").unwrap();
    let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(w.contains(r#"firstSheet="0""#) && w.contains(r#"activeTab="1""#), "{w}");
}

// ---- m4: move goes through core's checked move ----

#[test]
fn move_writes_sheet_elements_verbatim() {
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", "")], "", "");
    let before = text(&base, "xl/workbook.xml");
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.move_sheet(0, 2).unwrap();
    let after = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    let sheet = |w: &str, n: &str| {
        let i = w.find(&format!(r#"name="{n}""#)).unwrap();
        let s = w[..i].rfind("<sheet ").unwrap();
        let e = i + w[i..].find("/>").unwrap() + 2;
        w[s..e].to_owned()
    };
    for n in ["A", "B", "C"] {
        assert_eq!(sheet(&before, n), sheet(&after, n));
    }
    let order: Vec<usize> = ["B", "C", "A"].iter().map(|n| after.find(&format!(r#"name="{n}""#)).unwrap()).collect();
    assert!(order.is_sorted(), "{after}");
}

// ---- nit: [0]Sheet! self-references ----

#[test]
fn zero_index_self_reference_refuses() {
    let base = common::p1::book(&[("A", "<row r=\"1\"><c r=\"A1\"><f>[0]Data!A1</f></c></row>"), ("Data", "")], "", "");
    assert_matches!(refuses(&base, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
    assert_matches!(refuses(&base, |wb| wb.delete_sheet("Data")), Error::UnrewritableReference { .. });
}

// ---- M1: one reference in each row of ADR-0009 §6's table ----

/// One §6 location: the parts to add or replace (with content types) and
/// whether this version rewrites it (`true`) or refuses (`false`).
struct Row {
    label: &'static str,
    edits: Vec<(&'static str, String)>,
    types: Vec<(&'static str, &'static str)>,
    rewritten: bool,
}

const SHEET2: &str = "xl/worksheets/sheet2.xml";
const DRAWING_CT: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";

fn rows() -> Vec<Row> {
    let w = |inner: &str| String::from_utf8(ws(inner)).unwrap();
    let rels = |target: &str, ty: &str| {
        format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{ty}" Target="{target}"/></Relationships>"#)
    };
    let chart = |inner: &str| format!(r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:c15="http://schemas.microsoft.com/office/drawing/2012/chart"><c:chart>{inner}</c:chart></c:chartSpace>"#);
    vec![
        Row { label: "cell formula", edits: vec![(SHEET2, w(r#"<sheetData><row r="1"><c r="A1"><f>'Sheet 1'!A1+SUM(Data:'Sheet 1'!B1)</f><v>1</v></c></row></sheetData>"#))], types: vec![], rewritten: true },
        Row {
            label: "shared and array formulas",
            edits: vec![(SHEET2, w(r#"<sheetData><row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">'Sheet 1'!B1</f><v>1</v></c><c r="B1"><f t="array" ref="B1">'Sheet 1'!C1:C2</f><v>1</v></c></row></sheetData>"#))],
            types: vec![],
            rewritten: true,
        },
        Row { label: "chart c:f and c15:f", edits: vec![("xl/charts/chart1.xml", chart(r#"<c:f>'Sheet 1'!$A$1</c:f><c15:f>'Sheet 1'!$B$1</c15:f>"#))], types: vec![("xl/charts/chart1.xml", CHART_CT)], rewritten: true },
        Row { label: "c:pivotSource/c:name", edits: vec![("xl/charts/chart1.xml", chart(r#"<c:pivotSource><c:name>[b.xlsx]'Sheet 1'!PivotTable1</c:name></c:pivotSource>"#))], types: vec![("xl/charts/chart1.xml", CHART_CT)], rewritten: false },
        Row {
            label: "DV formula1",
            edits: vec![(SHEET2, w(r#"<sheetData/><dataValidations count="1"><dataValidation type="list" sqref="A1"><formula1>'Sheet 1'!$A$1:$A$3</formula1></dataValidation></dataValidations>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "CF formula",
            edits: vec![(SHEET2, w(r#"<sheetData/><conditionalFormatting sqref="A1"><cfRule type="expression" priority="1"><formula>'Sheet 1'!A1&gt;0</formula></cfRule></conditionalFormatting>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "x14 xm:f",
            edits: vec![(SHEET2, w(r#"<sheetData/><extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"><x14:dataValidations count="1"><x14:dataValidation><x14:formula1><xm:f>'Sheet 1'!$A$1</xm:f></x14:formula1></x14:dataValidation></x14:dataValidations></ext></extLst>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "table calculatedColumnFormula",
            edits: vec![("xl/tables/table9.xml", format!(r#"<table xmlns="{WS_NS}" id="9" name="T9" displayName="T9" ref="A1:A3"><tableColumns count="1"><tableColumn id="1" name="x"><calculatedColumnFormula>'Sheet 1'!A1</calculatedColumnFormula></tableColumn></tableColumns></table>"#))],
            types: vec![("xl/tables/table9.xml", TABLE_CT)],
            rewritten: false,
        },
        Row {
            label: "connections parameter/@cell",
            edits: vec![("xl/connections.xml", format!(r#"<connections xmlns="{WS_NS}"><connection id="1" name="q"><parameters count="1"><parameter name="p" cell="'Sheet 1'!$A$1"/></parameters></connection></connections>"#))],
            types: vec![("xl/connections.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml")],
            rewritten: false,
        },
        Row {
            label: "ctrlProps @fmlaLink",
            edits: vec![("xl/ctrlProps/ctrlProp1.xml", r#"<formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="CheckBox" fmlaLink="'Sheet 1'!$A$1"/>"#.into())],
            types: vec![("xl/ctrlProps/ctrlProp1.xml", "application/vnd.ms-excel.controlproperties+xml")],
            rewritten: false,
        },
        Row {
            label: "VML x:FmlaLink",
            edits: vec![("xl/drawings/vmlDrawing1.vml", r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:x="urn:schemas-microsoft-com:office:excel"><v:shape><x:ClientData ObjectType="Checkbox"><x:FmlaLink>'Sheet 1'!$A$1</x:FmlaLink></x:ClientData></v:shape></xml>"#.into())],
            types: vec![("xl/drawings/vmlDrawing1.vml", "application/vnd.openxmlformats-officedocument.vmlDrawing")],
            rewritten: false,
        },
        Row {
            label: "VML that does not parse",
            edits: vec![("xl/drawings/vmlDrawing1.vml", r#"<xml xmlns:x="urn:schemas-microsoft-com:office:excel"><br></xml><x:FmlaLink>'Sheet 1'!$A$1</x:FmlaLink>"#.into())],
            types: vec![("xl/drawings/vmlDrawing1.vml", "application/vnd.openxmlformats-officedocument.vmlDrawing")],
            rewritten: false,
        },
        Row {
            label: "xdr:sp/@textlink",
            edits: vec![("xl/drawings/drawing1.xml", r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"><xdr:sp textlink="'Sheet 1'!$A$1"/></xdr:wsDr>"#.into())],
            types: vec![("xl/drawings/drawing1.xml", DRAWING_CT)],
            rewritten: false,
        },
        Row { label: "hyperlink/@location", edits: vec![(SHEET2, w(r#"<sheetData/><hyperlinks><hyperlink ref="A1" location="'Sheet 1'!B2"/></hyperlinks>"#))], types: vec![], rewritten: true },
        Row {
            label: "shape hyperlink #target",
            edits: vec![
                ("xl/drawings/drawing1.xml", r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"/>"#.into()),
                ("xl/drawings/_rels/drawing1.xml.rels", rels("#'Sheet 1'!A1", "hyperlink")),
                (SHEET2, w(r#"<sheetData/><drawing r:id="rId1"/>"#)),
                ("xl/worksheets/_rels/sheet2.xml.rels", rels("../drawings/drawing1.xml", "drawing")),
            ],
            types: vec![("xl/drawings/drawing1.xml", DRAWING_CT)],
            rewritten: true,
        },
        // round 2 (R2-m3): the remaining rows of ADR §6's table, all refused
        Row { label: "chart c15:sqref", edits: vec![("xl/charts/chart1.xml", chart(r#"<c:extLst><c:ext uri="{X}"><c15:filteredLineSeries><c15:sqref>'Sheet 1'!$A$1</c15:sqref></c15:filteredLineSeries></c:ext></c:extLst>"#))], types: vec![("xl/charts/chart1.xml", CHART_CT)], rewritten: false },
        Row {
            label: "chartEx cx:f",
            edits: vec![("xl/charts/chartEx1.xml", r#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chartData><cx:data id="0"><cx:numDim type="val"><cx:f>'Sheet 1'!$A$1:$A$3</cx:f></cx:numDim></cx:data></cx:chartData></cx:chartSpace>"#.into())],
            types: vec![("xl/charts/chartEx1.xml", "application/vnd.ms-office.chartex+xml")],
            rewritten: false,
        },
        Row {
            label: "2007 cfvo/@val",
            edits: vec![(SHEET2, w(r#"<sheetData/><conditionalFormatting sqref="A1"><cfRule type="colorScale" priority="1"><colorScale><cfvo type="formula" val="'Sheet 1'!$A$1"/><cfvo type="max"/><color rgb="FF000000"/><color rgb="FFFFFFFF"/></colorScale></cfRule></conditionalFormatting>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "x14 sparkline xm:f",
            edits: vec![(SHEET2, w(r#"<sheetData/><extLst><ext uri="{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"><x14:sparklineGroups><x14:sparklineGroup><x14:sparklines><x14:sparkline><xm:f>'Sheet 1'!A1:E1</xm:f><xm:sqref>F1</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext></extLst>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "x14 CF xm:f",
            edits: vec![(SHEET2, w(r#"<sheetData/><extLst><ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}"><x14:conditionalFormattings><x14:conditionalFormatting><x14:cfRule type="expression" priority="1" id="{0}"><xm:f>'Sheet 1'!A1&gt;0</xm:f></x14:cfRule><xm:sqref>A1</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext></extLst>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "table totalsRowFormula",
            edits: vec![("xl/tables/table9.xml", format!(r#"<table xmlns="{WS_NS}" id="9" name="T9" displayName="T9" ref="A1:A3"><tableColumns count="1"><tableColumn id="1" name="x" totalsRowFunction="custom"><totalsRowFormula>SUM('Sheet 1'!A1)</totalsRowFormula></tableColumn></tableColumns></table>"#))],
            types: vec![("xl/tables/table9.xml", TABLE_CT)],
            rewritten: false,
        },
        Row {
            label: "controlPr/@linkedCell",
            edits: vec![(SHEET2, w(r#"<sheetData/><controls><control shapeId="1025" r:id="rId9" name="c"><controlPr linkedCell="'Sheet 1'!$A$1"/></control></controls>"#))],
            types: vec![],
            rewritten: false,
        },
        Row {
            label: "cdr:sp/@textlink",
            edits: vec![("xl/drawings/drawing9.xml", r#"<c:userShapes xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:cdr="http://schemas.openxmlformats.org/drawingml/2006/chartDrawing"><cdr:relSizeAnchor><cdr:sp textlink="'Sheet 1'!$A$1"/></cdr:relSizeAnchor></c:userShapes>"#.into())],
            types: vec![("xl/drawings/drawing9.xml", "application/vnd.openxmlformats-officedocument.drawingml.chartshapes+xml")],
            rewritten: false,
        },
        Row {
            label: "workbook extLst",
            edits: vec![("xl/workbook.xml", String::new())],
            types: vec![],
            rewritten: false,
        },
    ]
}

fn names_book() -> Vec<u8> {
    common::p1::book(
        &[("Sheet 1", ""), ("B", ""), ("Data", "")],
        r#"<definedName name="G">'Sheet 1'!$A$1</definedName><definedName name="L" localSheetId="1">'Sheet 1'!LocalName+'Sheet 1'!#REF!</definedName>"#,
        "",
    )
}

/// The fixture for one row; an empty `xl/workbook.xml` edit means "the
/// base workbook with the mention in an `extLst`" (review R2, X13).
fn row_input(row: &Row) -> Vec<u8> {
    let base = names_book();
    let mut edits: Vec<(&str, Vec<u8>)> = row
        .edits
        .iter()
        .map(|(p, b)| {
            let b = if *p == "xl/workbook.xml" && b.is_empty() {
                text(&base, p).replace("</workbook>", r#"<extLst><ext uri="{X}"><y>'Sheet 1'!A1</y></ext></extLst></workbook>"#)
            } else {
                b.clone()
            };
            (*p, b.into_bytes())
        })
        .collect();
    let ct: String = row.types.iter().map(|(p, t)| override_(p, t)).collect();
    if !ct.is_empty() {
        edits.push(ct_add(&base, &ct));
    }
    rebuild(&base, &edits)
}

#[test]
fn every_location_is_rewritten_or_refused() {
    for row in rows() {
        let input = row_input(&row);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        let r = wb.rename_sheet("Sheet 1", "New 1");
        let vi = residual::values(&input);
        let n_in = residual::residual(&vi, "Sheet 1").len();
        if !row.rewritten {
            assert_matches!(r, Err(Error::UnrewritableReference { .. } | Error::FormulaScanFailed { .. }), "{}", row.label);
            assert_unchanged(&wb, &input);
            continue;
        }
        let rw = r.unwrap_or_else(|e| panic!("{}: {e}", row.label));
        let out = wb.to_bytes().unwrap();
        let vo = residual::values(&out);
        let left = residual::residual(&vo, "Sheet 1");
        assert!(left.is_empty(), "{}: {left:#?}", row.label);
        assert_eq!(residual::residual(&vo, "New 1").len(), n_in, "{}", row.label);
        // the names (2 values) plus at least this row's location
        assert!(rw.len() > 2, "{}: {rw:?}", row.label);
        assert_eq!(Workbook::from_bytes(out).unwrap().__index_fingerprint(), wb.__index_fingerprint(), "{}", row.label);
    }
}

#[test]
fn every_location_after_a_delete() {
    for row in rows() {
        let input = row_input(&row);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        match wb.delete_sheet("Sheet 1") {
            Ok(_) => {
                assert!(row.rewritten, "{}: a location this version cannot rewrite was deleted around", row.label);
                let vo = residual::values(&wb.to_bytes().unwrap());
                let left: Vec<_> = residual::residual(&vo, "Sheet 1").into_iter().filter(|v| !v.location()).collect();
                assert!(left.is_empty(), "{}: {left:#?}", row.label);
            }
            Err(e) => {
                assert!(!row.rewritten, "{}: {e}", row.label);
                assert_unchanged(&wb, &input);
            }
        }
    }
}

// ---- M1: tests for the meaningful surviving mutants (review list) ----

fn wb_rels_add(base: &[u8], rel: &str) -> (&'static str, Vec<u8>) {
    ("xl/_rels/workbook.xml.rels", text(base, "xl/_rels/workbook.xml.rels").replace("</Relationships>", &format!("{rel}</Relationships>")).into_bytes())
}

/// R12: a structured reference may use the table's `displayName`.
#[test]
fn table_display_name_counts_as_a_use() {
    let base = table_book("", &[]);
    let t = format!(r#"<table xmlns="{WS_NS}" id="7" name="Inner" displayName="Shown" ref="A1:A3"><tableColumns count="1"><tableColumn id="1" name="x"/></tableColumns></table>"#);
    let input = rebuild(&base, &[("xl/tables/table1.xml", t.into_bytes()), ("xl/worksheets/sheet1.xml", ws(r#"<sheetData><row r="1"><c r="A1"><f>SUM(Shown[x])</f><v>0</v></c></row></sheetData>"#))]);
    assert_matches!(refuses(&input, |wb| wb.delete_sheet("B")), Error::TableInUse(_));
}

/// R13, R16: custom views name sheets by `sheetId`.
#[test]
fn custom_views_and_sheet_ids() {
    let views = |id: u32| format!(r#"<customWorkbookViews><customWorkbookView name="v" guid="{{00000000-0000-0000-0000-000000000001}}" windowWidth="1" windowHeight="1" activeSheetId="{id}"/></customWorkbookViews>"#);
    let base = common::p1::book(&[("A", ""), ("B", "")], "", &views(2));
    assert_matches!(refuses(&base, |wb| wb.delete_sheet("B")), Error::CustomViewReferencesSheet);
    let base = common::p1::book(&[("A", ""), ("B", "")], "", &views(9));
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.add_sheet("N", 2).unwrap();
    assert!(text(&wb.to_bytes().unwrap(), "xl/workbook.xml").contains(r#"name="N" sheetId="10""#));
}

/// R17: calcChain `i` values are sheet ids too.
#[test]
fn calc_chain_ids_are_not_reused() {
    let base = common::p1::book(&[("A", "<row r=\"1\"><c r=\"A1\"><f>1+1</f><v>2</v></c></row>")], "", "");
    let input = rebuild(&base, &[
        ("xl/calcChain.xml", format!(r#"<calcChain xmlns="{WS_NS}"><c r="A1" i="12"/></calcChain>"#).into_bytes()),
        wb_rels_add(&base, r#"<Relationship Id="rId90" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>"#),
        ct_add(&base, &override_("xl/calcChain.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml")),
    ]);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.add_sheet("N", 1).unwrap();
    assert!(text(&wb.to_bytes().unwrap(), "xl/workbook.xml").contains(r#"name="N" sheetId="13""#));
}

/// R14: ActiveX binaries may hold references this crate cannot read.
#[test]
fn activex_refuses_rename() {
    let base = two_sheets();
    let input = rebuild(&base, &[("xl/activeX/activeX1.bin", b"\0\0".to_vec()), ct_add(&base, &override_("xl/activeX/activeX1.bin", "application/vnd.ms-office.activeX"))]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::BinaryReferencesPossible);
}

/// R15, R32: chart formulas are escaped when spliced; a `c:f` with child
/// elements is not plain text and refuses.
#[test]
fn chart_formula_escaping_and_children() {
    let base = two_sheets();
    let chart = |inner: &str| format!(r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">{inner}</c:chartSpace>"#);
    let input = rebuild(&base, &[("xl/charts/chart1.xml", chart("<c:f>Data!$A$1</c:f>").into_bytes()), ct_add(&base, &override_("xl/charts/chart1.xml", CHART_CT))]);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.rename_sheet("Data", "A&B <x>").unwrap();
    let c = text(&wb.to_bytes().unwrap(), "xl/charts/chart1.xml");
    ooxml_core::XmlDocument::parse(c.as_bytes()).expect("chart parses");
    assert!(c.contains("<c:f>'A&amp;B &lt;x&gt;'!$A$1</c:f>"), "{c}");
    let input = rebuild(&base, &[("xl/charts/chart1.xml", chart("<c:f>Data!$A$1<c:x/></c:f>").into_bytes()), ct_add(&base, &override_("xl/charts/chart1.xml", CHART_CT))]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
}

/// R18, R31: a delete in manual calculation mode sets no fullCalcOnLoad;
/// `KeepStaleCaches` keeps the dependents' values.
#[test]
fn delete_respects_calc_mode_and_policy() {
    let sheets = [("A", "<row r=\"1\"><c r=\"A1\"><f>B!A1*2</f><v>4</v></c></row>"), ("B", "<row r=\"1\"><c r=\"A1\"><v>2</v></c></row>")];
    let base = common::p1::book(&sheets, "", r#"<calcPr calcId="1" calcMode="manual"/>"#);
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.delete_sheet("B").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(!text(&out, "xl/workbook.xml").contains("fullCalcOnLoad"));
    assert!(!text(&out, "xl/worksheets/sheet1.xml").contains("<v>4</v>"), "stripped by default");
    let base = common::p1::book(&sheets, "", "");
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.set_calc_policy(ooxml_xlsx::CalcPolicy::KeepStaleCaches);
    wb.delete_sheet("B").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/worksheets/sheet1.xml").contains("<v>4</v>"));
    assert!(text(&out, "xl/workbook.xml").contains(r#"fullCalcOnLoad="1""#));
}

/// R21, R22: a new sheet goes into the worksheets' folder and takes the
/// first visible sheet's default row height.
#[test]
fn add_follows_folder_and_row_height() {
    let base = common::p1::book(&[("A", "")], "", "");
    let rels = text(&base, "xl/_rels/workbook.xml.rels").replace("worksheets/sheet1.xml", "ws/sheet1.xml");
    let input = rebuild(&base, &[
        ("xl/ws/sheet1.xml", ws(r#"<sheetFormatPr defaultRowHeight="20.25"/><sheetData/>"#)),
        ("xl/_rels/workbook.xml.rels", rels.into_bytes()),
        ct_add(&base, &override_("xl/ws/sheet1.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml")),
    ]);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.add_sheet("N", 1).unwrap();
    let out = wb.to_bytes().unwrap();
    let pkg = ooxml_core::Package::from_bytes(out).unwrap();
    let new: Vec<&str> = pkg.part_names().filter(|p| p.starts_with("xl/ws/") && *p != "xl/ws/sheet1.xml").collect();
    assert_eq!(new.len(), 1, "{:?}", pkg.part_names().collect::<Vec<_>>());
    let x = String::from_utf8(pkg.read_part(new[0]).unwrap().to_vec()).unwrap();
    assert!(x.contains(r#"defaultRowHeight="20.25""#), "{x}");
}

/// R24: 31 UTF-16 units at most.
#[test]
fn name_length_limit() {
    let mut wb = Workbook::from_bytes(two_sheets()).unwrap();
    assert_matches!(wb.rename_sheet("Data", &"x".repeat(32)), Err(Error::InvalidSheetName { .. }));
    wb.rename_sheet("Data", &"x".repeat(31)).unwrap();
}

/// R27: a workbook the Phase1 census refused cannot be restructured.
#[test]
fn census_block_refuses_every_operation() {
    let base = two_sheets();
    let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(r#"<sheetData/><extLst><ext uri="{X}"><f>1</f></ext></extLst>"#))]);
    assert_matches!(refuses(&input, |wb| wb.add_sheet("N", 0).map(|_| vec![])), Error::FormulaScanFailed { .. });
    assert_matches!(refuses(&input, |wb| wb.move_sheet(0, 1).map(|_| vec![])), Error::FormulaScanFailed { .. });
}

/// R29: a move remaps `localSheetId`.
#[test]
fn move_remaps_local_names() {
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", "")], r#"<definedName name="L" localSheetId="0">A!$A$1</definedName>"#, "");
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.move_sheet(0, 2).unwrap();
    assert!(text(&wb.to_bytes().unwrap(), "xl/workbook.xml").contains(r#"<definedName name="L" localSheetId="2">"#));
}

/// R30: hiding the active sheet moves `tabSelected` to the new active one.
#[test]
fn hide_moves_tab_selected() {
    let sel = r#"<sheetViews><sheetView tabSelected="1" workbookViewId="0"/></sheetViews><sheetData/>"#;
    let plain = r#"<sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetData/>"#;
    let base = common::p1::book(&[("A", ""), ("B", "")], "", r#"<bookViews><workbookView activeTab="0"/></bookViews>"#);
    let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(sel)), ("xl/worksheets/sheet2.xml", ws(plain))]);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.set_sheet_state("A", ooxml_xlsx::SheetState::Hidden).unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(!text(&out, "xl/worksheets/sheet1.xml").contains("tabSelected"));
    assert!(text(&out, "xl/worksheets/sheet2.xml").contains(r#"tabSelected="1""#));
}

/// R33: a pivot cache that does not parse but names the sheet refuses.
#[test]
fn unparseable_pivot_cache_refuses_delete() {
    let input = with_pivot(&two_sheets(), &pivot(r#"<worksheetSource ref="A1:B3" sheet="Data"/>"#).replace("<cacheFields", "<!DOCTYPE x><cacheFields"));
    assert_matches!(refuses(&input, |wb| wb.delete_sheet("Data")), Error::SheetIsPivotSource(_));
}

/// R34: a sheet with parts outside the cascade table refuses deletion.
#[test]
fn unsupported_sheet_parts_refuse_delete() {
    let base = two_sheets();
    let input = rebuild(&base, &[
        (
            "xl/worksheets/_rels/sheet2.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#.to_vec(),
        ),
        ("xl/pivotTables/pivotTable1.xml", format!(r#"<pivotTableDefinition xmlns="{WS_NS}" name="P"/>"#).into_bytes()),
        ct_add(&base, &override_("xl/pivotTables/pivotTable1.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml")),
    ]);
    assert_matches!(refuses(&input, |wb| wb.delete_sheet("Data")), Error::SheetHasUnsupportedParts { .. });
}

// ---- round 2 (docs/reviews/pr53-round2.md) ----

/// R2-B1: raw names are matched after XML and ST_Xstring decoding.
#[test]
fn raw_names_with_entities() {
    for (sheet, attr) in [("R&D", "R&amp;D"), ("開発&設計", "開発&amp;設計"), ("It's", "It&apos;s"), ("Data", "D&#97;ta"), ("a<b\"c", "a&lt;b&quot;c")] {
        let base = common::p1::book(&[("A", ""), (&sheet.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;"), "")], "", "");
        let input = with_pivot(&base, &pivot(&format!(r#"<worksheetSource ref="A1:B3" sheet="{attr}"/>"#)));
        let mut wb = Workbook::from_bytes(input).unwrap();
        assert_eq!(wb.sheet_names().nth(1), Some(sheet));
        let rw = wb.rename_sheet(sheet, "New").unwrap();
        assert!(rw.iter().any(|r| r.location == "worksheetSource sheet" && r.before == sheet), "{sheet}: {rw:?}");
        assert!(text(&wb.to_bytes().unwrap(), "xl/pivotCache/pivotCacheDefinition1.xml").contains(r#"sheet="New""#), "{sheet}");
        // consolidation and dataRef refuse
        let cons = r#"<consolidation><rangeSets count="1"><rangeSet ref="A1:B3" sheet="ATTR"/></rangeSets></consolidation>"#.replace("ATTR", attr);
        let input = with_pivot(&base, &pivot(&cons).replace(r#"type="worksheet""#, r#"type="consolidation""#));
        assert_matches!(refuses(&input, |wb| wb.rename_sheet(sheet, "New")), Error::UnrewritableReference { .. }, "{sheet}");
        let dr = format!(r#"<sheetData/><dataConsolidate><dataRefs count="1"><dataRef ref="A1:B3" sheet="{attr}"/></dataRefs></dataConsolidate>"#);
        let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(&dr))]);
        assert_matches!(refuses(&input, |wb| wb.rename_sheet(sheet, "New")), Error::UnrewritableReference { .. }, "{sheet}");
    }
}

/// R2-B1: this crate's own output round-trips: rename to `P&L` (and a
/// name with `<`, `"` and LF), then rename again.
#[test]
fn raw_names_round_trip() {
    for mid in ["P&L", "a<b\"c", "x\ny"] {
        let input = with_pivot(&two_sheets(), &pivot(r#"<worksheetSource ref="A1:B3" sheet="Data"/>"#));
        let mut wb = Workbook::from_bytes(input).unwrap();
        wb.rename_sheet("Data", mid).unwrap();
        let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
        assert_eq!(wb.sheet_names().nth(1), Some(mid));
        let rw = wb.rename_sheet(mid, "Back").unwrap();
        assert!(rw.iter().any(|r| r.location == "worksheetSource sheet"), "{mid:?}");
        assert!(text(&wb.to_bytes().unwrap(), "xl/pivotCache/pivotCacheDefinition1.xml").contains(r#"sheet="Back""#), "{mid:?}");
    }
}

/// Unsupported extension content must not inherit the scan exemptions for
/// chart formulas, worksheet cell values, hyperlinks or relationship targets.
#[test]
fn scan_exemptions_require_the_supported_namespace_and_path() {
    let base = two_sheets();
    let chart = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:extLst><c:ext uri="{X}"><other:f xmlns:other="urn:extension">Data!A1</other:f></c:ext></c:extLst></c:chartSpace>"#;
    let chart_part = "xl/charts/chart1.xml";
    let input = rebuild(&base, &[(chart_part, chart.to_vec()), ct_add(&base, &override_(chart_part, CHART_CT))]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });

    for extension in [r#"<v>Data!A1</v>"#, r#"<hyperlink ref="A1" location="Data!A1"/>"#] {
        let sheet = format!(r#"<sheetData/><extLst><ext uri="{{X}}">{extension}</ext></extLst>"#);
        let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(&sheet))]);
        assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
    }

    // The census recognizes the immediate cell parent in this extension,
    // while the rewriter visits only the worksheet's top-level sheetData.
    let sheet = r#"<sheetData/><extLst><ext uri="{X}"><sheetData><row r="1"><c r="A1"><f>Data!A1</f></c></row></sheetData></ext></extLst>"#;
    let input = rebuild(&base, &[("xl/worksheets/sheet1.xml", ws(sheet))]);
    assert!(Workbook::from_bytes(input.clone()).unwrap().edit_refusal().is_none());
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });

    let relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="Data!A1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="other.xlsx" TargetMode="External"/></Relationships>"#;
    let input = rebuild(&base, &[("xl/worksheets/_rels/sheet1.xml.rels", relationships.to_vec())]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
}

#[test]
fn structural_edits_update_the_shared_string_count() {
    let sst = format!(r#"<sst xmlns="{WS_NS}" count="2" uniqueCount="1"><si><t>x</t></si></sst>"#);
    let base = common::p1::book(
        &[
            ("A", r#"<row r="1"><c r="A1" t="s"><f>Data!A1</f><v>0</v></c></row>"#),
            ("Data", r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#),
        ],
        "",
        "",
    );
    let rels = text(&base, "xl/_rels/workbook.xml.rels").replace(
        "</Relationships>",
        concat!(r#"<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#, "</Relationships>"),
    );
    let input = rebuild(&base, &[
        ("xl/_rels/workbook.xml.rels", rels.into_bytes()),
        ("xl/sharedStrings.xml", sst.into_bytes()),
        ct_add(&base, &override_("xl/sharedStrings.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml")),
    ]);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet_by_name("A").unwrap().cell_mut(0, 1).unwrap().set_string("x").unwrap();
    wb.delete_sheet("Data").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/sharedStrings.xml").contains(r#"count="1""#));
    assert!(text(&out, "xl/worksheets/sheet1.xml").contains("<f>#REF!</f>"));

    wb.worksheet_by_name("A").unwrap().cell_mut(0, 2).unwrap().set_string("x").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/sharedStrings.xml").contains(r#"count="2""#));
}

/// R2-m1: an External relationship with a `#Sheet!` target is not
/// retargeted, so it refuses.
#[test]
fn external_fragment_targets_refuse() {
    let rels = r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#Data!A1" TargetMode="External"/></Relationships>"##;
    let input = rebuild(&two_sheets(), &[("xl/worksheets/_rels/sheet1.xml.rels", rels.as_bytes().to_vec())]);
    assert_matches!(refuses(&input, |wb| wb.rename_sheet("Data", "New")), Error::UnrewritableReference { .. });
    // another file's sheet is not ours
    let other = rels.replace("#Data!A1", "other.xlsx#Data!A1");
    let input = rebuild(&two_sheets(), &[("xl/worksheets/_rels/sheet1.xml.rels", other.into_bytes())]);
    Workbook::from_bytes(input).unwrap().rename_sheet("Data", "New").unwrap();
}

/// R2-m2: a pivot source through a chain of names.
#[test]
fn pivot_source_through_a_name_chain_refuses_delete() {
    let dn = r#"<definedName name="Src">Data!$A$1:$B$3</definedName><definedName name="Src2">Src</definedName><definedName name="Loop1">Loop2</definedName><definedName name="Loop2">Loop1</definedName>"#;
    let base = common::p1::book(&[("A", ""), ("Data", "")], dn, "");
    let input = with_pivot(&base, &pivot(r#"<worksheetSource name="Src2"/>"#));
    assert_matches!(refuses(&input, |wb| wb.delete_sheet("Data")), Error::SheetIsPivotSource(_));
    let input = with_pivot(&base, &pivot(r#"<worksheetSource name="Loop1"/>"#));
    Workbook::from_bytes(input).unwrap().delete_sheet("Data").unwrap();
}

/// X9: strict delete counts an Internal `#Sheet!` relationship target.
#[test]
fn strict_counts_internal_fragment_targets() {
    let rels = r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#Data!A1"/></Relationships>"##;
    let input = rebuild(&two_sheets(), &[("xl/worksheets/_rels/sheet1.xml.rels", rels.as_bytes().to_vec())]);
    assert_matches!(refuses(&input, |wb| wb.delete_sheet_strict("Data")), Error::SheetInUse { count: 1, .. });
}

/// R2-m4: a name holding `_xHHHH_` literally is protected on write and
/// reads back as given.
#[test]
fn xstring_names_round_trip() {
    let mut wb = Workbook::from_bytes(two_sheets()).unwrap();
    wb.rename_sheet("A", "x_x0041_y").unwrap();
    assert_eq!(wb.sheet_names().next(), Some("x_x0041_y"));
    assert!(text(&wb.to_bytes().unwrap(), "xl/workbook.xml").contains(r#"name="x_x005F_x0041_y""#));
    let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    assert_eq!(wb.sheet_names().next(), Some("x_x0041_y"));
    wb.rename_sheet("x_x0041_y", "Z").unwrap();
}

/// Review R2 nits: an external pivot source (`r:id`) does not block a
/// delete; another workbook's names in an external link do not block a
/// rename.
#[test]
fn other_workbooks_do_not_refuse() {
    let input = with_pivot(&two_sheets(), &pivot(r#"<worksheetSource ref="A1:B3" sheet="Data" r:id="rId1"/>"#));
    Workbook::from_bytes(input).unwrap().delete_sheet("Data").unwrap();
    let base = two_sheets();
    let el = format!(r#"<externalLink xmlns="{WS_NS}"><externalBook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:id="rId1"><definedNames><definedName name="X" refersTo="=Data!$A$1"/></definedNames></externalBook></externalLink>"#);
    let input = rebuild(&base, &[("xl/externalLinks/externalLink1.xml", el.into_bytes()), ct_add(&base, &override_("xl/externalLinks/externalLink1.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml"))]);
    Workbook::from_bytes(input).unwrap().rename_sheet("Data", "New").unwrap();
}
