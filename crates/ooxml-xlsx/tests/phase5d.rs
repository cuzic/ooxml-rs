//! Sparkline and dynamic-array write oracles (ADR-0017 §§8–9).

mod common;

use std::assert_matches;

use common::p1::{SML, book, cell_spans, synthetic, with_part};
use common::{assert_l1_except, entries, part, read};
use ooxml_core::{Package, PartNameRequest, QName, RawNode, XmlDocument};
use ooxml_xlsx::{
    __formula_parses, CellIndex, CellRange, CellValue, Error, SparklineGroup, SparklineType, Workbook, Worksheet,
};

const SHEET: &str = "xl/worksheets/sheet1.xml";
const META: &str = "xl/metadata.xml";
const RELS: &str = "xl/_rels/workbook.xml.rels";
const CT: &str = "[Content_Types].xml";
const X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const XM: &str = "http://schemas.microsoft.com/office/excel/2006/main";
const XDA: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray";
const URI: &str = "{05C60535-1F16-4fd2-B633-F4F36F0B64E0}";
const SORT: &str = "_xlfn._xlws.SORT(A1:A3)";
const XLNS: &str = "http://www.w3.org/2000/xmlns/";

fn attr<'a>(node: &'a RawNode, name: &'static str) -> Option<&'a str> {
    node.attr(QName::new("", name))
}

fn child<'a>(node: &'a RawNode, ns: &'static str, name: &'static str) -> &'a RawNode {
    node.elements()
        .find(|e| e.is(QName::new(ns, name)))
        .unwrap()
}

fn xml(bytes: &[u8], path: &str) -> XmlDocument {
    XmlDocument::parse(&part(bytes, path)).unwrap()
}

fn source<'a>(doc: &'a XmlDocument, node: &RawNode) -> &'a str {
    let span = node.span().unwrap();
    &doc.source()[span.start..span.end()]
}

fn erase(doc: &XmlDocument, node: &RawNode) -> String {
    doc.source().replacen(source(doc, node), "", 1)
}

fn spark(ws: &mut Worksheet<'_>, dest: &str, source: &str) -> ooxml_xlsx::Result<()> {
    ws.add_sparklines(SparklineGroup::new(SparklineType::Line).add(dest, source))
}

fn write_dynamic(ws: &mut Worksheet<'_>, anchor: &str, text: &str) -> ooxml_xlsx::Result<()> {
    let at = CellIndex::parse(anchor)?;
    ws.cell_mut(at.row, at.col)?.set_dynamic_formula(text)
}

fn verify_anchor(bytes: &[u8], sheet: &str, at: &str, text: &str) -> u32 {
    let doc = xml(bytes, sheet);
    let cell = child(doc.root(), SML, "sheetData")
        .elements()
        .flat_map(RawNode::elements)
        .find(|c| attr(c, "r") == Some(at))
        .unwrap();
    let formula = child(cell, SML, "f");
    assert_eq!(attr(formula, "t"), Some("array"));
    assert_eq!(attr(formula, "ref"), Some(at));
    assert_eq!(formula.text(), text);
    assert!(__formula_parses(text));
    assert!(
        !cell
            .elements()
            .any(|e| matches!(e.name().local(), "v" | "is"))
    );
    attr(cell, "cm").unwrap().parse().unwrap()
}

fn assert_existing_entries(before: &[u8], after: &[u8], changed: &[&str]) {
    let parts = entries(after);
    for entry in entries(before) {
        if !changed.contains(&entry.0.as_str()) {
            assert!(parts.contains(&entry), "changed {}", entry.0);
        }
    }
}

#[test]
fn sparkline_uses_local_namespaces_and_appends_to_existing_extension() {
    let input = book(&[("O'Brien &amp; Co", "")], "", "");
    let sheet = format!(
        r#"<worksheet xmlns="{SML}" xmlns:legacy="urn:legacy"><sheetData /><extLst><ext uri='other'><legacy:keep a='original' /></ext></extLst></worksheet>"#
    );
    let input = with_part(&input, SHEET, &sheet);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    spark(&mut wb.worksheet().unwrap(), "D1", "'O''Brien & Co'!A1:C1").unwrap();

    let first = wb.to_bytes().unwrap();
    assert_l1_except(&input, &first, &[SHEET]);
    let doc = xml(&first, SHEET);
    let list = child(doc.root(), SML, "extLst");
    let exts: Vec<_> = list.elements().collect();
    assert_eq!(exts.len(), 2);
    assert_eq!(attr(exts[1], "uri"), Some(URI));
    assert_eq!(erase(&doc, exts[1]), sheet);
    assert_eq!(exts[1].attr(QName::new(XLNS, "x14")), Some(X14));
    assert_eq!(exts[1].attr(QName::new(XLNS, "xm")), None);
    let groups = child(exts[1], X14, "sparklineGroups");
    assert_eq!(groups.attr(QName::new(XLNS, "xm")), Some(XM));
    let old_group = child(groups, X14, "sparklineGroup");
    let series = child(child(old_group, X14, "sparklines"), X14, "sparkline");
    let formula = child(series, XM, "f").text();
    assert_eq!(formula, "'O''Brien & Co'!A1:C1");
    assert!(__formula_parses(&formula));
    assert_eq!(child(series, XM, "sqref").text(), "D1");

    spark(&mut wb.worksheet().unwrap(), "E1", "'O''Brien & Co'!A1:C1").unwrap();
    let second = wb.to_bytes().unwrap();
    assert_l1_except(&first, &second, &[SHEET]);
    let out = xml(&second, SHEET);
    let list = child(out.root(), SML, "extLst");
    assert_eq!(list.elements().count(), 2);
    let groups = child(list.elements().last().unwrap(), X14, "sparklineGroups");
    assert_eq!(groups.elements().count(), 2);
    assert_eq!(
        source(&out, groups.elements().next().unwrap()),
        source(&doc, old_group)
    );
    assert_eq!(erase(&out, groups.elements().last().unwrap()), doc.source());
}

#[test]
fn sparkline_groups_can_share_a_destination() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    for range in ["Sheet1!A1:C1", "Sheet1!A2:C2"] {
        spark(&mut ws, "D1", range).unwrap();
    }

    let out = wb.to_bytes().unwrap();
    let doc = xml(&out, SHEET);
    let ext = child(child(doc.root(), SML, "extLst"), SML, "ext");
    let groups = child(ext, X14, "sparklineGroups");
    let series: Vec<_> = groups
        .elements()
        .map(|group| {
            let sparkline = child(child(group, X14, "sparklines"), X14, "sparkline");
            (
                child(sparkline, XM, "sqref").text(),
                child(sparkline, XM, "f").text(),
            )
        })
        .collect();
    assert_eq!(series.len(), 2);
    for ((dest, source), expected) in series.iter().zip(["Sheet1!A1:C1", "Sheet1!A2:C2"]) {
        assert_eq!(dest, "D1");
        assert_eq!(source, expected);
    }
}

#[test]
fn sparkline_options_use_the_x14_schema() {
    use ooxml_xlsx::{SparklineAxis, SparklineEmptyCells};

    let mut wb = Workbook::new().unwrap();
    let mut group = SparklineGroup::new(SparklineType::Stacked).add("B1", "Sheet1!A1:A3");
    group.markers = true;
    group.high = true;
    group.low = true;
    group.first = true;
    group.last = true;
    group.negative = true;
    group.display_x_axis = true;
    group.min_axis = SparklineAxis::Custom(-2.0);
    group.max_axis = SparklineAxis::Group;
    group.empty_cells = SparklineEmptyCells::Span;
    group.colors.series = Some(0xFF01_2345);
    group.colors.high = Some(0x80FF_0000);
    wb.worksheet().unwrap().add_sparklines(group).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let doc = xml(&bytes, SHEET);
    let ext = child(child(doc.root(), SML, "extLst"), SML, "ext");
    let group = child(child(ext, X14, "sparklineGroups"), X14, "sparklineGroup");
    for name in [
        "markers",
        "high",
        "low",
        "first",
        "last",
        "negative",
        "displayXAxis",
    ] {
        assert_eq!(attr(group, name), Some("1"));
    }
    for (name, value) in [
        ("type", "stacked"),
        ("displayEmptyCellsAs", "span"),
        ("minAxisType", "custom"),
        ("manualMin", "-2"),
        ("maxAxisType", "group"),
    ] {
        assert_eq!(attr(group, name), Some(value));
    }
    assert_eq!(
        attr(child(group, X14, "colorSeries"), "rgb"),
        Some("FF012345")
    );
    assert_eq!(
        attr(child(group, X14, "colorHigh"), "rgb"),
        Some("80FF0000")
    );
    let series = child(child(group, X14, "sparklines"), X14, "sparkline");
    assert_eq!(child(series, XM, "f").text(), "Sheet1!A1:A3");
}

#[test]
fn dynamic_array_invalidates_potential_spill_dependents_and_registers_metadata() {
    let target = r#"<row r="1"><c r="A1"><v>3</v></c><c r="C1"><v>10</v></c><c r="D1"><v>7</v></c><c r="E1"><f>D2*2</f><v>12</v></c></row>"#;
    let other = r#"<row r="1"><c r="A1"><f>Sheet1!D3*2</f><v>18</v></c><c r="B1"><f>Sheet1!A1*2</f><v>6</v></c></row>"#;
    let input = book(&[("Sheet1", target), ("Sheet2", other)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    write_dynamic(&mut wb.worksheet().unwrap(), "C1", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_existing_entries(
        &input,
        &out,
        &[
            CT,
            RELS,
            SHEET,
            "xl/worksheets/sheet2.xml",
            "xl/workbook.xml",
        ],
    );
    assert_eq!(verify_anchor(&out, SHEET, "C1", SORT), 1);

    for (registry, closing) in [(CT, "</Types>"), (RELS, "</Relationships>")] {
        let before = xml(&input, registry);
        let after = xml(&out, registry);
        let appended = after.root().elements().last().unwrap();
        assert!(
            after
                .source()
                .ends_with(&format!("{}{closing}", source(&after, appended)))
        );
        assert_eq!(erase(&after, appended), before.source());
    }
    let before = xml(&input, "xl/workbook.xml");
    let after = xml(&out, "xl/workbook.xml");
    let calc = child(after.root(), SML, "calcPr");
    assert_eq!(attr(calc, "fullCalcOnLoad"), Some("1"));
    assert_eq!(erase(&after, calc), before.source());

    let doc = xml(&out, META);
    assert_eq!(doc.root().attr(QName::new(XLNS, "xda")), None);
    let future = child(doc.root(), SML, "futureMetadata");
    let ext = child(child(child(future, SML, "bk"), SML, "extLst"), SML, "ext");
    assert_eq!(ext.attr(QName::new(XLNS, "xda")), Some(XDA));

    for (sheet, changes) in [
        (SHEET, &[("C1", "<v>10</v>"), ("E1", "<v>12</v>")][..]),
        ("xl/worksheets/sheet2.xml", &[("A1", "<v>18</v>")][..]),
    ] {
        let before = cell_spans(&part(&input, sheet));
        let after = cell_spans(&part(&out, sheet));
        assert_eq!(before.len(), after.len());
        for (at, span) in before {
            if sheet == SHEET && at.to_string() == "C1" {
                continue;
            }
            let cache = changes
                .iter()
                .find(|(cell, _)| at.to_string() == *cell)
                .map(|(_, cache)| *cache);
            assert_eq!(
                after[&at],
                cache.map_or(span.clone(), |v| span.replace(v, ""))
            );
        }
    }

    let mut reopened = Workbook::from_bytes(out.clone()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    assert_matches!(
        ws.cell_mut(0, 2).unwrap().set_number(0.0),
        Err(Error::ArrayAnchor { .. })
    );
    assert_eq!(reopened.to_bytes().unwrap(), out);
    reopened
        .worksheet()
        .unwrap()
        .cell_mut(1, 2)
        .unwrap()
        .set_number(42.0)
        .unwrap();
}

#[test]
fn rich_value_metadata_keeps_original_order_indices_and_bytes() {
    let input = read("calamine/pictures_in_cell_and_over_cell.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let mut ws = wb.worksheet().unwrap();
    let sheet = ws.part_name().to_owned();
    write_dynamic(&mut ws, "K1", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_l1_except(&input, &out, &[&sheet, META, "xl/workbook.xml"]);
    assert_eq!(verify_anchor(&out, &sheet, "K1", SORT), 1);
    let before = xml(&input, META);
    let after = xml(&out, META);
    let children: Vec<_> = after.root().elements().map(|e| e.name().local()).collect();
    assert_eq!(
        children,
        [
            "metadataTypes",
            "futureMetadata",
            "futureMetadata",
            "cellMetadata",
            "valueMetadata"
        ]
    );
    let types = child(after.root(), SML, "metadataTypes");
    assert_eq!(attr(types, "count"), Some("2"));
    let added = types.elements().last().unwrap();
    let future = after
        .root()
        .elements()
        .find(|n| attr(n, "name") == Some("XLDAPR"))
        .unwrap();
    let cells = child(after.root(), SML, "cellMetadata");
    assert_eq!(attr(future, "count"), Some("1"));
    assert_eq!(attr(cells, "count"), Some("1"));
    assert_eq!(
        attr(child(child(cells, SML, "bk"), SML, "rc"), "t"),
        Some("2")
    );
    let restored = after
        .source()
        .replace(source(&after, added), "")
        .replace(source(&after, future), "")
        .replace(source(&after, cells), "")
        .replace(r#"metadataTypes count="2""#, r#"metadataTypes count="1""#);
    assert_eq!(restored, before.source());
    let values = child(after.root(), SML, "valueMetadata").elements().count();
    for (path, _, _, _) in entries(&out)
        .into_iter()
        .filter(|e| e.0.starts_with("xl/worksheets/") && e.0.ends_with(".xml"))
    {
        let doc = xml(&out, &path);
        for index in child(doc.root(), SML, "sheetData")
            .elements()
            .flat_map(RawNode::elements)
            .filter_map(|c| attr(c, "vm"))
            .map(|v| v.parse::<usize>().unwrap())
        {
            assert!((1..=values).contains(&index));
        }
    }
}

#[test]
fn metadata_reuses_only_active_blocks_and_updates_counts() {
    let input = read("poi/xlookup.xlsx");
    let metadata = xml(&input, META);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let sheet = wb.worksheet().unwrap().part_name().to_owned();
    write_dynamic(&mut wb.worksheet().unwrap(), "K1", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(verify_anchor(&out, &sheet, "K1", SORT), 1);
    assert_eq!(part(&out, META), part(&input, META));

    let blank = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(blank).unwrap();
    write_dynamic(&mut wb.worksheet().unwrap(), "K1", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    for changed in [
        r#"fDynamic="0" fCollapsed="0""#,
        r#"fDynamic="1" fCollapsed="1""#,
    ] {
        let doc = xml(&out, META);
        let edited = doc
            .source()
            .replace(r#"fDynamic="1" fCollapsed="0""#, changed);
        assert_ne!(edited, doc.source());
        let input = with_part(&out, META, &edited);
        let input = with_part(
            &input,
            SHEET,
            &format!(r#"<worksheet xmlns="{SML}"><sheetData/></worksheet>"#),
        );
        let mut wb = Workbook::from_bytes(input).unwrap();
        write_dynamic(&mut wb.worksheet().unwrap(), "K1", SORT).unwrap();
        let out = wb.to_bytes().unwrap();
        assert_eq!(verify_anchor(&out, SHEET, "K1", SORT), 2);
        let doc = xml(&out, META);
        let future = child(doc.root(), SML, "futureMetadata");
        let cells = child(doc.root(), SML, "cellMetadata");
        assert_eq!(attr(future, "count"), Some("2"));
        assert_eq!(attr(cells, "count"), Some("2"));
        assert_eq!(
            attr(child(cells.elements().last().unwrap(), SML, "rc"), "v"),
            Some("1")
        );
        assert!(doc.source().contains(changed));
        assert!(Workbook::from_bytes(out).unwrap().edit_refusal().is_none());
    }
    assert!(metadata.source().contains("XLDAPR"));
}

#[test]
fn collapsed_dynamic_anchor_allows_edits_without_reusing_its_block() {
    let mut wb = Workbook::new().unwrap();
    write_dynamic(&mut wb.worksheet().unwrap(), "C1", SORT).unwrap();
    let input = wb.to_bytes().unwrap();
    let before = xml(&input, META);
    let collapsed = before
        .source()
        .replace(r#"fCollapsed="0""#, r#"fCollapsed="1""#);
    assert_ne!(collapsed, before.source());
    let input = with_part(&input, META, &collapsed);

    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet()
        .unwrap()
        .cell_mut(0, 5)
        .unwrap()
        .set_number(42.0)
        .unwrap();
    let edited = wb.to_bytes().unwrap();
    assert_eq!(part(&edited, META), collapsed.as_bytes());
    assert_eq!(verify_anchor(&edited, SHEET, "C1", SORT), 1);

    let mut wb = Workbook::from_bytes(edited).unwrap();
    write_dynamic(&mut wb.worksheet().unwrap(), "E1", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(verify_anchor(&out, SHEET, "C1", SORT), 1);
    assert_eq!(verify_anchor(&out, SHEET, "E1", SORT), 2);
    let doc = xml(&out, META);
    let future = child(doc.root(), SML, "futureMetadata");
    assert_eq!(attr(future, "count"), Some("2"));
    let old = future.elements().next().unwrap();
    let new = future.elements().nth(1).unwrap();
    assert!(source(&doc, old).contains(r#"fCollapsed="1""#));
    assert!(source(&doc, new).contains(r#"fCollapsed="0""#));
    let cells = child(doc.root(), SML, "cellMetadata");
    let record = child(cells.elements().nth(1).unwrap(), SML, "rc");
    assert_eq!(attr(record, "v"), Some("1"));
    assert!(Workbook::from_bytes(out).unwrap().edit_refusal().is_none());
}

#[test]
fn dynamic_array_registers_a_new_part_beside_orphan_metadata() {
    let workbook = book(&[("Sheet1", "")], "", "");
    let orphan = format!(r#"<metadata xmlns="{SML}"><!-- orphan --></metadata>"#);
    let mut pkg = Package::from_bytes(workbook).unwrap();
    pkg.add_part(
        PartNameRequest::Exact(META.into()),
        "application/xml",
        orphan.as_bytes().to_vec(),
    )
    .unwrap();
    let input = pkg.to_bytes().unwrap();
    assert_matches!(
        refusal(&input, |ws| write_dynamic(ws, "C1", "SORT(A1:A3)")),
        Error::MissingFunctionPrefix { .. }
    );
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    write_dynamic(&mut wb.worksheet().unwrap(), "C1", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_existing_entries(&input, &out, &[CT, RELS, SHEET, "xl/workbook.xml"]);
    assert_eq!(part(&out, META), orphan.as_bytes());

    let ns = "http://schemas.openxmlformats.org/package/2006/content-types";
    let ct = xml(&out, CT);
    let rels = xml(&out, RELS);
    let rel_type =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata";
    let mut metadata_rels = rels
        .root()
        .elements()
        .filter(|rel| attr(rel, "Type") == Some(rel_type));
    let target = attr(metadata_rels.next().unwrap(), "Target").unwrap();
    assert!(metadata_rels.next().is_none());
    assert_ne!(target, "metadata.xml");
    let path = format!("xl/{target}");
    let absolute = format!("/{path}");
    let override_ = ct
        .root()
        .elements()
        .find(|e| {
            e.is(QName::new(ns, "Override")) && attr(e, "PartName") == Some(absolute.as_str())
        })
        .unwrap();
    assert_eq!(
        attr(override_, "ContentType"),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml")
    );
    let metadata = xml(&out, &path);
    assert_eq!(
        attr(child(metadata.root(), SML, "cellMetadata"), "count"),
        Some("1")
    );
    assert_eq!(verify_anchor(&out, SHEET, "C1", SORT), 1);
    assert!(Workbook::from_bytes(out).unwrap().edit_refusal().is_none());
}

fn refusal(input: &[u8], edit: impl FnOnce(&mut Worksheet<'_>) -> ooxml_xlsx::Result<()>) -> Error {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let before = wb.to_bytes().unwrap();
    let err = edit(&mut wb.worksheet().unwrap()).unwrap_err();
    assert!(
        wb.to_bytes().unwrap() == before,
        "{err} changed the workbook"
    );
    assert_l1_except(input, &before, &[]);
    err
}

#[test]
fn refusals_preserve_every_byte() {
    let blank = Workbook::new().unwrap().to_bytes().unwrap();
    assert_matches!(
        refusal(&blank, |ws| write_dynamic(ws, "C1", "SORT(A1:A3)")),
        Error::MissingFunctionPrefix { .. }
    );
    for text in ["A1#", "@A1"] {
        assert_matches!(
            refusal(&blank, |ws| write_dynamic(ws, "C1", text)),
            Error::DynamicArrayOperator { .. }
        );
    }
    assert_matches!(
        refusal(&blank, |ws| spark(ws, "D1", "Missing!A1:C1")),
        Error::NoSuchSheet(_)
    );
    assert_matches!(
        refusal(&blank, |ws| spark(ws, "D1", "Sheet1!A1:$?")),
        Error::InvalidFormula { .. }
    );
    assert_matches!(
        refusal(&blank, |ws| spark(ws, "D1:E1", "Sheet1!A1:C1")),
        Error::InvalidReference(_)
    );
    assert_matches!(
        refusal(&blank, |ws| ws
            .add_sparklines(SparklineGroup::new(SparklineType::Stacked))),
        Error::InvalidSparklineGroup
    );

    let ac = format!(
        r#"<worksheet xmlns="{SML}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:x14="{X14}"><sheetData/><mc:AlternateContent><mc:Choice Requires="x14"><extLst/></mc:Choice></mc:AlternateContent></worksheet>"#
    );
    let input = with_part(&blank, SHEET, &ac);
    assert_matches!(
        refusal(&input, |ws| spark(ws, "D1", "Sheet1!A1:C1")),
        Error::Core(ooxml_core::Error::AlreadyPresent(_))
    );

    let implicit = book(&[("Sheet1", r#"<row r="4"><c><v>1</v></c></row>"#)], "", "");
    assert_matches!(
        refusal(&implicit, |ws| write_dynamic(ws, "C1", SORT)),
        Error::MissingReference { .. }
    );
    let shared = book(
        &[(
            "Sheet1",
            r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">B1+1</f><v>1</v></c></row><row r="2"><c r="A2"><f t="shared" si="0"/><v>2</v></c></row>"#,
        )],
        "",
        "",
    );
    assert_matches!(
        refusal(&shared, |ws| write_dynamic(ws, "A1", SORT)),
        Error::SharedFormulaMaster { .. }
    );
    let rich = read("calamine/pictures_in_cell_and_over_cell.xlsx");
    let mut wb = Workbook::from_bytes(rich.clone()).unwrap();
    let sheet = xml(&rich, wb.worksheet().unwrap().part_name());
    let vm = child(sheet.root(), SML, "sheetData")
        .elements()
        .flat_map(RawNode::elements)
        .find(|c| attr(c, "vm").is_some())
        .unwrap();
    assert_matches!(
        refusal(&rich, |ws| write_dynamic(ws, attr(vm, "r").unwrap(), SORT)),
        Error::RichValueCell { .. }
    );

    let input = read("poi/xlookup.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let sheet = xml(&input, wb.worksheet().unwrap().part_name());
    let cell = child(sheet.root(), SML, "sheetData")
        .elements()
        .flat_map(RawNode::elements)
        .find(|c| attr(c, "cm").is_some())
        .unwrap();
    assert_matches!(
        refusal(&input, |ws| write_dynamic(
            ws,
            attr(cell, "r").unwrap(),
            SORT
        )),
        Error::ArrayAnchor { .. }
    );
    let wrong_root = format!(r#"<unexpected xmlns="{SML}"/>"#);
    let input = with_part(&rich, META, &wrong_root);
    assert_matches!(
        refusal(&input, |ws| write_dynamic(ws, "K1", SORT)),
        Error::PartNotEditable { .. }
    );
}

#[test]
fn dynamic_array_removes_calc_chain() {
    let input = synthetic("o1_precise.xlsx");
    let mut wb = Workbook::from_bytes(input).unwrap();
    assert!(wb.package().contains("xl/calcChain.xml"));
    write_dynamic(&mut wb.worksheet_by_name("Sheet1").unwrap(), "B10", SORT).unwrap();
    let out = wb.to_bytes().unwrap();
    let wb = Workbook::from_bytes(out).unwrap();
    assert!(!wb.package().contains("xl/calcChain.xml"));
}

#[cfg(feature = "fault-injection")]
#[test]
fn apply_faults_poison_but_stale_plans_do_not() {
    use ooxml_xlsx::ApplyStep;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    type Edit = fn(&mut Worksheet<'_>) -> ooxml_xlsx::Result<()>;
    let dynamic: Edit = |ws| write_dynamic(ws, "C1", SORT);
    let sparkline: Edit = |ws| spark(ws, "D1", "Sheet1!A1:C1");
    for (edit, steps) in [
        (
            dynamic,
            &[
                ApplyStep::Registry,
                ApplyStep::Strip,
                ApplyStep::HandOff,
                ApplyStep::Registries,
                ApplyStep::Target,
                ApplyStep::Index,
            ][..],
        ),
        (sparkline, &[ApplyStep::Registry, ApplyStep::Sparklines][..]),
    ] {
        for &step in steps {
            for panic in [false, true] {
                let mut wb = Workbook::new().unwrap();
                if panic {
                    wb.inject_apply_panic(step);
                } else {
                    wb.inject_apply_failure(step);
                }
                let run = catch_unwind(AssertUnwindSafe(|| edit(&mut wb.worksheet().unwrap())));
                if panic {
                    assert!(run.is_err(), "{step:?}");
                } else {
                    assert_matches!(run.unwrap(), Err(Error::Poisoned(_)));
                }
                assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
            }
        }
        let mut wb = Workbook::new().unwrap();
        let before = wb.to_bytes().unwrap();
        wb.inject_apply_failure(ApplyStep::StaleRegistryPlan);
        assert_matches!(
            edit(&mut wb.worksheet().unwrap()),
            Err(Error::Core(ooxml_core::Error::RegistryRefused {
                reason: ooxml_core::RegistryRefusal::StalePlan,
                ..
            }))
        );
        assert_eq!(wb.to_bytes().unwrap(), before);
    }
}

fn verify_extension_append(before: &[u8], after: &[u8], sheet: &str, name: &str, dest: &str) {
    assert_l1_except(before, after, &[sheet]);
    let old = xml(before, sheet);
    let new = xml(after, sheet);
    let list = child(new.root(), SML, "extLst");
    let matching = |n: &&RawNode| attr(n, "uri") == Some(URI);
    let ext = list.elements().find(matching).unwrap();
    let groups = child(ext, X14, "sparklineGroups");
    let added = groups.elements().last().unwrap();
    let spark = child(child(added, X14, "sparklines"), X14, "sparkline");
    let formula = child(spark, XM, "f").text();
    let (prefix, range) = formula.rsplit_once('!').unwrap();
    assert_eq!(range, "A1:A3");
    assert_eq!(
        prefix
            .strip_circumfix("'", "'")
            .map_or_else(|| prefix.to_owned(), |n| n.replace("''", "'")),
        name
    );
    assert_eq!(child(spark, XM, "sqref").text(), dest);
    assert!(__formula_parses(&formula));
    if let Some(old_list) = old
        .root()
        .elements()
        .find(|n| n.is(QName::new(SML, "extLst")))
    {
        if let Some(old_ext) = old_list.elements().find(matching) {
            if old_ext
                .elements()
                .any(|n| n.is(QName::new(X14, "sparklineGroups")))
            {
                let old_groups = child(old_ext, X14, "sparklineGroups");
                let residual = erase(&new, added);
                if old_groups.span().unwrap().close.is_some() {
                    assert_eq!(residual, old.source());
                } else {
                    // A self-closing container now has an explicit end tag.
                    assert_eq!(erase(&old, old_groups), erase(&new, groups));
                }
            } else if old_ext.span().unwrap().close.is_some() {
                assert_eq!(erase(&new, groups), old.source());
            } else {
                assert_eq!(erase(&old, old_ext), erase(&new, ext));
            }
        } else if old_list.span().unwrap().close.is_some() {
            assert_eq!(erase(&new, ext), old.source());
        } else {
            assert_eq!(erase(&old, old_list), erase(&new, list));
        }
    } else {
        assert_eq!(erase(&new, list), old.source());
    }
}

#[test]
fn preserves_excel_authored_x14_conditional_formatting() {
    let input = read("poi/NewStyleConditionalFormattings.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let mut ws = wb.worksheet().unwrap();
    let sheet = ws.part_name().to_owned();
    let name = ws.name().to_owned();
    let before = xml(&input, &sheet);
    let ext = child(before.root(), SML, "extLst");
    assert!(source(&before, ext).contains("conditionalFormattings"));
    spark(&mut ws, "Z1", &format!("'{name}'!A1:A3")).unwrap();
    let out = wb.to_bytes().unwrap();
    verify_extension_append(&input, &out, &sheet, &name, "Z1");
}

#[test]
#[ignore = "needs a local corpus: OOXML_XLSX_CORPUS=<dir>"]
fn corpus_sweep() {
    use std::collections::BTreeMap;

    let dir = std::env::var("OOXML_XLSX_CORPUS").expect("OOXML_XLSX_CORPUS");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "xlsx" || ext == "xlsm")
        })
        .collect();
    files.sort();
    let mut results = BTreeMap::new();
    for path in files {
        let input = std::fs::read(&path).unwrap();
        let Ok(mut wb) = Workbook::from_bytes(input.clone()) else {
            *results.entry("open refusal".into()).or_insert(0) += 1;
            continue;
        };
        let baseline = wb.to_bytes().unwrap();
        let Ok(mut ws) = wb.worksheet() else {
            *results.entry("worksheet refusal".into()).or_insert(0) += 1;
            continue;
        };
        let sheet = ws.part_name().to_owned();
        let name = ws.name().to_owned();
        let source = format!("'{}'!A1:A3", name.replace('\'', "''"));
        let dest = (1..=32)
            .map(|r| format!("XFC{r}"))
            .find(|a1| matches!(ws.cell_a1(a1), Ok(None)));
        let anchor = (1..=32)
            .map(|r| format!("XFD{r}"))
            .find(|a1| matches!(ws.cell_a1(a1), Ok(None)));
        let (Some(dest), Some(anchor)) = (dest, anchor) else {
            *results
                .entry("could not locate two empty cells".into())
                .or_insert(0) += 1;
            continue;
        };
        let result = spark(&mut ws, &dest, &source);
        let after_spark = wb.to_bytes().unwrap();
        match result {
            Ok(()) => {
                *results.entry("sparklines success".into()).or_insert(0) += 1;
                verify_extension_append(&input, &after_spark, &sheet, &name, &dest);
                Workbook::from_bytes(after_spark.clone()).unwrap();
            }
            Err(error) => {
                let kind = format!("{error:?}");
                *results
                    .entry(format!(
                        "sparklines refusal {}",
                        kind.split([' ', '(']).next().unwrap()
                    ))
                    .or_insert(0) += 1;
                assert!(after_spark == baseline, "{}", path.display());
            }
        }
        let mut wb = Workbook::from_bytes(after_spark.clone()).unwrap();
        let result = write_dynamic(&mut wb.worksheet().unwrap(), &anchor, SORT);
        let after_dynamic = wb.to_bytes().unwrap();
        match result {
            Ok(()) => {
                *results.entry("dynamic success".into()).or_insert(0) += 1;
                verify_anchor(&after_dynamic, &sheet, &anchor, SORT);
                let mut reopened = Workbook::from_bytes(after_dynamic.clone()).unwrap();
                assert!(
                    reopened.edit_refusal().is_none(),
                    "{}: {:?}",
                    path.display(),
                    reopened.edit_refusal()
                );
                let at = CellIndex::parse(&anchor).unwrap();
                let r = reopened
                    .worksheet()
                    .unwrap()
                    .cell_mut(at.row, at.col)
                    .unwrap()
                    .set_number(0.0);
                assert_matches!(r, Err(Error::ArrayAnchor { .. }));
                assert_eq!(reopened.to_bytes().unwrap(), after_dynamic);
            }
            Err(error) => {
                let kind = format!("{error:?}");
                *results
                    .entry(format!(
                        "dynamic refusal {}",
                        kind.split([' ', '(']).next().unwrap()
                    ))
                    .or_insert(0) += 1;
                assert!(after_dynamic == after_spark, "{}", path.display());
            }
        }
    }
    for (result, count) in results {
        eprintln!("{result}: {count}");
    }
}


#[test]
fn static_array_write_and_range_guard() {
    let input = synthetic("o1_arrays.xlsx");
    let mut wb = Workbook::from_bytes(input).unwrap();
    let range = CellRange::parse("H2:I3").unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(range.first.row, range.first.col).unwrap()
        .set_array_formula("1+2", range, false).unwrap();
    assert!(ws.array_formulas().contains(&(range.first, range, "1+2".into(), false)));
    let written = wb.to_bytes().unwrap();
    let mut reopened = Workbook::from_bytes(written.clone()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    assert!(ws.array_formulas().contains(&(range.first, range, "1+2".into(), false)));
    let overlap = CellRange::parse("G1:H2").unwrap();
    assert_matches!(ws.cell_mut(overlap.first.row, overlap.first.col).unwrap()
        .set_array_formula("2+3", overlap, false), Err(Error::ArrayResultCell { .. }));
    assert_eq!(reopened.to_bytes().unwrap(), written);
}


#[test]
fn ordinary_formula_cache_is_validated_and_invalidated() {
    let mut book = Workbook::new_at(std::time::UNIX_EPOCH).unwrap();
    let original = book.to_bytes().unwrap();
    let mut sheet = book.worksheet().unwrap();
    assert_matches!(sheet.cell_mut(0, 1).unwrap().set_formula_with_result("A1+2", CellValue::Number(f64::NAN)),
        Err(Error::InvalidNumber(_)));
    assert_eq!(book.to_bytes().unwrap(), original);
    let mut sheet = book.worksheet().unwrap();
    sheet.cell_mut(0, 1).unwrap().set_formula_with_result("A1+2", CellValue::Number(3.0)).unwrap();
    assert_eq!(sheet.cell(0, 1).unwrap().unwrap().value().unwrap(), CellValue::Number(3.0));
    sheet.cell_mut(0, 0).unwrap().set_number(2.0).unwrap();
    assert_eq!(sheet.cell(0, 1).unwrap().unwrap().value().unwrap(), CellValue::Empty);
}

#[test]
fn array_anchor_cache_is_validated_and_invalidated() {
    let input = synthetic("o1_arrays.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let range = CellRange::parse("H2:I3").unwrap();
    let mut ws = wb.worksheet().unwrap();
    assert_matches!(
        ws.cell_mut(1, 7).unwrap()
            .set_array_formula_with_result("Z1+2", range, false, CellValue::Number(f64::INFINITY)),
        Err(Error::InvalidNumber(_))
    );
    assert_eq!(wb.to_bytes().unwrap(), input);

    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(1, 7).unwrap()
        .set_array_formula_with_result("Z1+2", range, false, CellValue::Number(3.0))
        .unwrap();
    assert_eq!(ws.cell(1, 7).unwrap().unwrap().value().unwrap(), CellValue::Number(3.0));
    ws.cell_mut(0, 25).unwrap().set_number(9.0).unwrap();
    assert_eq!(ws.cell(1, 7).unwrap().unwrap().value().unwrap(), CellValue::Empty);
}
