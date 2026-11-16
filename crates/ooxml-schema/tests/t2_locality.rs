//! T2 (locality) oracle of ADR-0001 at the foundation level: generic lens
//! edits through the generated SML specs on the fixture's `sheet1.xml`.
//! Every byte-level diff must stay inside the edited cell's original span,
//! or be exactly the insertion at the row/column-ordered position (plus the
//! `<dimension>` span when that is updated too). No format-specific domain
//! API is used: these helpers stand in for what `ooxml-xlsx` will do.

use ooxml_core::{EnumValue, Error, LensMut, Package, QName, RawNode, XmlDocument};
use ooxml_schema::sml::*;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/round_trip_corpus/fixture_minimal.xlsx");
const SHEET1: &str = "xl/worksheets/sheet1.xml";

fn sheet1() -> (String, XmlDocument) {
    let pkg = Package::open(FIXTURE).unwrap();
    let bytes = pkg.read_part(SHEET1).unwrap();
    (String::from_utf8(bytes.clone()).unwrap(), XmlDocument::parse(&bytes).unwrap())
}

fn col_of(r: &str) -> u32 {
    r.bytes().take_while(u8::is_ascii_alphabetic).fold(0, |a, b| a * 26 + u32::from(b - b'A' + 1))
}
fn row_of(r: &str) -> u32 {
    r.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse().unwrap()
}
fn attr_r(n: &RawNode) -> &str {
    n.attr(QName::new("", "r")).unwrap()
}

/// Finds or creates `<row r=…>` and `<c r=…>` in row/column order.
fn cell_mut<'d>(sd: &'d mut LensMut<'_, SheetData>, r: &str) -> Result<CellViewMut<'d>, Error> {
    let (row, col) = (row_of(r), col_of(r));
    let row_s = row.to_string();
    let exists = sd.as_lens().children::<Row>().any(|x| x.attr(Row::ATTR_R) == Some(row_s.as_str()));
    let mut row_lens = if exists {
        sd.child_mut_by::<Row>(|n| attr_r(n) == row_s).unwrap()
    } else {
        let mut new = sd.insert_by::<Row>(|_, n| attr_r(n).parse::<u32>().unwrap() > row)?;
        new.set_attr(Row::ATTR_R, &row_s)?;
        new
    };
    let existing = row_lens
        .as_lens()
        .children::<Cell>()
        .find(|c| c.attr(Cell::ATTR_R) == Some(r))
        .map(|c| *c.path().last().unwrap());
    let idx = match existing {
        Some(i) => i,
        None => {
            let mut c = row_lens.insert_by::<Cell>(|_, n| col_of(attr_r(n)) > col)?;
            c.set_attr(Cell::ATTR_R, r)?;
            *c.path().last().unwrap()
        }
    };
    Ok(CellViewMut(row_lens.into_child_at::<Cell>(idx).unwrap()))
}

fn set_number(c: &mut CellViewMut<'_>, v: &str) -> Result<(), Error> {
    c.remove_data_type()?;
    c.remove_inline_string();
    c.get_or_insert_cell_value()?.set_text(v)
}

fn set_string(c: &mut CellViewMut<'_>, v: &str) -> Result<(), Error> {
    c.set_data_type(CellValues::InlineString)?;
    c.remove_cell_value();
    let mut is = c.get_or_insert_inline_string()?;
    is.get_or_insert_text()?.set_text(v)
}

/// Changed window of `out` vs `input` (common prefix/suffix removed),
/// as a range of `input`.
fn changed(input: &str, out: &str) -> std::ops::Range<usize> {
    let p = input.bytes().zip(out.bytes()).take_while(|(a, b)| a == b).count();
    let s = input.bytes().rev().zip(out.bytes().rev()).take_while(|(a, b)| a == b).count();
    let s = s.min(input.len() - p).min(out.len() - p);
    p..input.len() - s
}

fn cell_span(doc: &XmlDocument, r: &str) -> std::ops::Range<usize> {
    let ws = WorksheetView(doc.lens::<Worksheet>().unwrap());
    let sd = ws.sheet_data().unwrap();
    for row in sd.row_iter() {
        for c in row.cell_iter() {
            if c.cell_reference() == Some(r) {
                let s = c.raw().span().unwrap();
                return s.start..s.end();
            }
        }
    }
    panic!("no cell {r}")
}

#[test]
fn t2_every_existing_cell_same_and_different_value() {
    let (input, original) = sheet1();
    let cells = ["A1", "A2", "B2", "C2", "D2"];
    for r in cells {
        let span = cell_span(&original, r);
        for (label, edit) in [
            ("same raw value", 0),
            ("different number", 1),
            ("string (inlineStr)", 2),
        ] {
            let mut doc = original.clone();
            {
                let mut ws = doc.lens_mut::<Worksheet>().unwrap();
                let mut sd = ws.child_mut::<SheetData>().unwrap();
                let mut c = cell_mut(&mut sd, r).unwrap();
                match edit {
                    0 => {
                        let current = c.as_view().cell_value().map(|v| v.text().into_owned());
                        if let Some(v) = current {
                            c.get_or_insert_cell_value().unwrap().set_text(&v).unwrap();
                        }
                    }
                    1 => set_number(&mut c, "42.5").unwrap(),
                    _ => set_string(&mut c, " x & y ").unwrap(),
                }
            }
            let out = doc.serialize();
            let w = changed(&input, &out);
            assert!(
                w.is_empty() || (w.start >= span.start && w.end <= span.end),
                "{r} {label}: diff {w:?} escapes the cell span {span:?}\n{out}"
            );
            if edit == 0 {
                assert_eq!(out, input, "{r}: writing the same value must not change bytes");
            }
            let reparsed = XmlDocument::parse(out.as_bytes()).unwrap();
            let view = WorksheetView(reparsed.lens::<Worksheet>().unwrap());
            let cell = view.sheet_data().unwrap().row_iter().flat_map(|row| row.cell_iter().collect::<Vec<_>>()).find(|c| c.cell_reference() == Some(r)).unwrap();
            match edit {
                1 => {
                    assert_eq!(cell.data_type(), EnumValue::Known(CellValues::Number), "{r}: t removed (C2's t=\"b\" too)");
                    assert_eq!(cell.cell_value().unwrap().text(), "42.5");
                }
                2 => {
                    assert_eq!(cell.data_type(), EnumValue::Known(CellValues::InlineString));
                    let t = cell.inline_string().unwrap().text().unwrap();
                    assert_eq!(t.text(), " x & y ");
                    assert_eq!(t.space(), Some(EnumValue::Known(SpaceProcessingModeValues::Preserve)));
                }
                _ => {}
            }
        }
    }
}

#[test]
fn t2_self_closing_cell_gains_a_value() {
    let (input, mut doc) = sheet1();
    {
        let mut ws = doc.lens_mut::<Worksheet>().unwrap();
        let mut sd = ws.child_mut::<SheetData>().unwrap();
        let mut c = cell_mut(&mut sd, "D2").unwrap();
        set_number(&mut c, "7").unwrap();
    }
    let out = doc.serialize();
    assert_eq!(out, input.replace(r#"<c r="D2" s="1"/>"#, r#"<c r="D2" s="1"><v>7</v></c>"#));
}

#[test]
fn t2_new_cells_and_rows_go_to_their_ordered_insertion_point() {
    let (input, original) = sheet1();
    let cases: &[(&str, &str, &str)] = &[
        // (new cell, expected text inserted, inserted before this anchor in the input).
        // Appending puts the node immediately before the parent's close tag,
        // after any whitespace (ADR-0002 §4.2); no whitespace is added.
        ("B1", r#"<c r="B1"><v>5</v></c>"#, "</row>\n<row r=\"2\">"),
        ("E2", r#"<c r="E2"><v>5</v></c>"#, "</row>\n</sheetData>"),
        ("A3", r#"<row r="3"><c r="A3"><v>5</v></c></row>"#, "</sheetData>"),
    ];
    for &(r, inserted, anchor) in cases {
        let mut doc = original.clone();
        {
            let mut ws = doc.lens_mut::<Worksheet>().unwrap();
            let mut sd = ws.child_mut::<SheetData>().unwrap();
            let mut c = cell_mut(&mut sd, r).unwrap();
            set_number(&mut c, "5").unwrap();
        }
        let at = input.find(anchor).unwrap();
        let expected = format!("{}{inserted}{}", &input[..at], &input[at..]);
        assert_eq!(doc.serialize(), expected, "{r}");
    }

    // an existing string cell becomes a number, plus the <dimension> update
    let mut doc = original.clone();
    {
        let ws = doc.lens_mut::<Worksheet>().unwrap();
        let mut ws = WorksheetViewMut(ws);
        {
            let mut sd = ws.sheet_data_mut().unwrap();
            let mut c = cell_mut(&mut sd.0, "A2").unwrap();
            set_number(&mut c, "1").unwrap();
        }
        ws.get_or_insert_sheet_dimension().unwrap().set_reference("A1:F9").unwrap();
    }
    let expected = input
        .replace(r#"<c r="A2" t="s"><v>1</v></c>"#, r#"<c r="A2"><v>1</v></c>"#)
        .replace(r#"<dimension ref="A1:D2"/>"#, r#"<dimension ref="A1:F9"/>"#);
    assert_eq!(doc.serialize(), expected);
}

#[test]
fn reads_use_xsd_defaults_and_never_coerce() {
    let (_, doc) = sheet1();
    let ws = WorksheetView(doc.lens::<Worksheet>().unwrap());
    assert_eq!(ws.sheet_dimension().unwrap().reference(), Some("A1:D2"));
    let rows: Vec<_> = ws.sheet_data().unwrap().row_iter().collect();
    assert_eq!(rows.iter().map(|r| r.row_index().unwrap()).collect::<Vec<_>>(), [Some(1), Some(2)]);
    let cells: Vec<_> = rows[1].cell_iter().collect();
    assert_eq!(cells[1].style_index(), Ok(0), "absent s = default 0");
    assert_eq!(cells[1].data_type(), EnumValue::Known(CellValues::Number), "absent t = default n");
    assert_eq!(cells[2].data_type(), EnumValue::Known(CellValues::Boolean));
    assert_eq!(cells[3].style_index(), Ok(1));
    let bad = XmlDocument::parse(br#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" s="x" t="future"/>"#).unwrap();
    let c = CellView(bad.lens::<Cell>().unwrap());
    assert_eq!(c.style_index().unwrap_err().raw, "x");
    assert_eq!(c.data_type(), EnumValue::Unknown("future"));
    // workbook and shared strings: read-only views
    let mut pkg = Package::open(FIXTURE).unwrap();
    let wb = pkg.xml_part("xl/workbook.xml").unwrap().clone();
    let wbv = WorkbookView(wb.lens::<Workbook>().unwrap());
    let first = wbv.sheets().unwrap().sheet_iter().next().unwrap();
    assert_eq!(first.name(), Some("Sheet1"));
    assert_eq!(first.id(), Some("rId1"));
    let sst = pkg.xml_part("xl/sharedStrings.xml").unwrap();
    let sstv = SharedStringTableView(sst.lens::<SharedStringTable>().unwrap());
    let texts: Vec<String> = sstv.shared_string_item_iter().map(|si| si.text().unwrap().text().into_owned()).collect();
    assert_eq!(texts, ["Name", "Alice"]);
}

#[test]
fn formula_and_unknown_children_are_not_insertable_by_type() {
    // `f` is read-only in the allowlist: no `get_or_insert_cell_formula` on CellViewMut.
    // ChildOf still holds, so the generic primitive works (domain crates refuse formula writes).
    let mut doc = XmlDocument::parse(br#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><v>1</v></c>"#).unwrap();
    let mut c = doc.lens_mut::<Cell>().unwrap();
    c.insert::<CellFormula>().unwrap();
    assert_eq!(doc.serialize(), r#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><f/><v>1</v></c>"#);
}
