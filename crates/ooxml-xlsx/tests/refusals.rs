//! Expected refusals: docs/roadmap/xlsx.md §7.5. A refused call changes
//! nothing: the workbook still saves to the input's entries (L1).

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::PartStateKind;
use ooxml_xlsx::{CellValue, Error, Workbook};

/// P0-R1..R3 superseded by Phase1 (ADR-0003): formula-bearing workbooks are
/// no longer refused workbook-wide; reads still return cached values and
/// formula text. (Phase1 behaviour is tested in `phase1_*.rs`.)
#[test]
fn r1_to_r3_formula_workbooks_are_editable_in_phase1() {
    for name in [
        "poi/shared_formulas.xlsx",
        "poi/70005-countifs.xlsx",
        "poi/MatrixFormulaEvalTestData.xlsx",
        "poi/xlookup.xlsx",
        "poi/NewStyleConditionalFormattings.xlsx",
    ] {
        let wb = Workbook::from_bytes(read(name)).unwrap();
        assert!(wb.edit_refusal().is_none(), "{name}: {:?}", wb.edit_refusal());
    }
    let mut wb = Workbook::from_bytes(read("poi/shared_formulas.xlsx")).unwrap();
    let ws = wb.worksheet().unwrap();
    let c = ws.cell(1, 0).unwrap().unwrap();
    assert_eq!(c.formula().as_deref(), Some("B2"));
    assert_eq!(c.value().unwrap(), CellValue::String("ProductionOrderConfirmation".into()));
}

/// P0-R4, R5: rows/cells without `r` → reads and writes refused.
#[test]
fn r4_r5_missing_reference() {
    for name in ["poi/59746_NoRowNums.xlsx", "poi/InlineString.xlsx"] {
        let input = read(name);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        {
            let ws = wb.worksheet().unwrap();
            assert_matches!(ws.cell(0, 0), Err(Error::MissingReference { .. }), "{name}");
        }
        let (wb, r) = apply(&input, a1("A1"), &W::Num(1.0));
        assert_matches!(r, Err(Error::MissingReference { .. }), "{name}");
        assert_unchanged(&wb, &input);
    }
}

/// P0-R6, relaxed by ADR-0014 §3: a value write to a non-anchor cell of a
/// merged range is refused; the anchor (top-left) is writable, as in Excel.
#[test]
fn r6_merged() {
    let input = read("poi/61605.xlsx");
    let merges = merged(&input);
    for cell in ["B1", "C1", "E1", "B9"] {
        let at = a1(cell);
        let anchor = merges.iter().any(|m| m.first == at);
        let (wb, r) = apply(&input, at, &W::Num(1.0));
        if anchor {
            r.unwrap_or_else(|e| panic!("{cell}: the anchor is writable: {e}"));
        } else {
            assert_matches!(r, Err(Error::MergedCell { ref range, .. }) if range.starts_with('B'), "{cell}");
            assert_unchanged(&wb, &input);
        }
    }
    assert!(["B1", "B9"].iter().any(|c| merges.iter().any(|m| m.first == a1(c))), "the test covers an anchor");
}

/// P0-R7: `cell_mut` without a setter changes nothing at all.
#[test]
fn r7_cell_mut_without_setter() {
    let input = read("poi/61605.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let _ = ws.cell_mut(19, 0).unwrap();
        let _ = ws.cell_mut(1, 1).unwrap();
        let _ = ws.cell(1, 1).unwrap();
    }
    assert_unchanged(&wb, &input);
}

/// P0-R8: there is no `set_formula` in Phase0; formula writes are not
/// expressible. (Compile-time: `CellMut` has only `set_string`/`set_number`.)
#[test]
fn r8_no_formula_write_api() {
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(0, 0).unwrap().set_string_inline("=1+1").unwrap();
    assert_eq!(ws.cell(0, 0).unwrap().unwrap().formula(), None, "a leading = is text, not a formula");
}

const WS_OPEN: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#;

fn with_sheet(xml: &str) -> Vec<u8> {
    let mut pkg = ooxml_core::Package::from_bytes(read("minimal")).unwrap();
    pkg.set_part("xl/worksheets/sheet1.xml", xml.as_bytes().to_vec()).unwrap();
    pkg.to_bytes().unwrap()
}

/// P0-R9 (adapted: a missing `dimension` is never inserted, coordinator
/// default): a worksheet with an `mc:AlternateContent` child and no
/// `dimension` is writable; one that also lacks `sheetData` needs an
/// insertion next to the AC. The Choice's unbound Requires prefix
/// makes that insertion fail with `UnresolvedSibling`.
#[test]
fn r9_alternate_content_sibling() {
    let ac = "<mc:AlternateContent><mc:Choice Requires=\"x14\"><controls/></mc:Choice></mc:AlternateContent>";
    let ok = with_sheet(&format!("{WS_OPEN}<sheetData/>{ac}</worksheet>"));
    let (wb, r) = apply(&ok, a1("B2"), &W::Num(1.0));
    r.unwrap();
    let xml = String::from_utf8(part(&wb.to_bytes().unwrap(), "xl/worksheets/sheet1.xml")).unwrap();
    assert!(xml.contains(r#"<sheetData><row r="2"><c r="B2"><v>1</v></c></row></sheetData>"#) && !xml.contains("dimension"), "{xml}");
    let bad = with_sheet(&format!("{WS_OPEN}{ac}</worksheet>"));
    let (wb, r) = apply(&bad, a1("B2"), &W::Num(1.0));
    assert_matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), Workbook::from_bytes(bad).unwrap().to_bytes().unwrap());
    assert_eq!(wb.package().state("xl/worksheets/sheet1.xml"), Some(PartStateKind::Parsed { dirty: false }));
}

/// Rows out of order, a cell whose `r` names another row, out-of-range
/// indices, and a byte-pattern `<f`-lookalike that is not a formula.
#[test]
fn malformed_shapes() {
    let rows = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="3"/><row r="2"/></sheetData></worksheet>"#));
    assert_matches!(apply(&rows, a1("A1"), &W::Num(1.0)).1, Err(Error::OutOfOrder(_)));
    let wrong = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="1"><c r="A2"/></row></sheetData></worksheet>"#));
    assert_matches!(apply(&wrong, a1("A1"), &W::Num(1.0)).1, Err(Error::InvalidReference(_)));
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    let mut ws = wb.worksheet().unwrap();
    assert_matches!(ws.cell_mut(1 << 20, 0), Err(Error::OutOfRange { .. }));
    assert_matches!(ws.cell(0, 1 << 14), Err(Error::OutOfRange { .. }));
    let fill = with_sheet(&format!(r#"{WS_OPEN}<sheetData/><filterColumn/><formula/></worksheet>"#));
    assert!(Workbook::from_bytes(fill).unwrap().edit_refusal().is_none());
    let f = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="1"><c r="A1"><f>1</f></c></row></sheetData></worksheet>"#));
    assert_matches!(apply(&f, a1("C3"), &W::Num(1.0)).1, Ok(()), "no dependent: a Phase1 value edit");
    let dtd = with_sheet(&format!(r#"<!DOCTYPE x>{WS_OPEN}<sheetData/></worksheet>"#));
    assert_matches!(Workbook::from_bytes(dtd), Err(Error::Core(ooxml_core::Error::DtdNotAllowed { .. })));
}

/// Reads: shared strings, rich inline strings, booleans, errors, styles.
#[test]
fn reads() {
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    let ws = wb.worksheet().unwrap();
    let v = |r, c| ws.cell(r, c).unwrap().map(|c| c.value().unwrap());
    assert_eq!(v(0, 0), Some(CellValue::String("Name".into())));
    assert_eq!(v(1, 1), Some(CellValue::Number(90.0)));
    assert_eq!(v(1, 2), Some(CellValue::Bool(true)));
    assert_eq!(v(1, 3), Some(CellValue::Empty));
    assert_eq!(v(5, 5), None);
    assert_eq!(ws.cell_a1("A1").unwrap().unwrap().style_index().unwrap(), 1);
    let mut wb = Workbook::from_bytes(read("calamine/pictures_in_cell_and_over_cell.xlsx")).unwrap();
    let ws = wb.worksheet().unwrap();
    assert_eq!(ws.cell(0, 0).unwrap().unwrap().value().unwrap(), CellValue::Error("#VALUE!".into()));
    // every readable cell of every openable fixture decodes, and agrees with calamine
    for name in all_zip_fixtures() {
        let input = read(&name);
        let Ok(mut wb) = Workbook::from_bytes(input.clone()) else { continue };
        let sheet = wb.sheet_name().to_owned();
        let Some(cal) = try_calamine_cells(&input) else { continue }; // calamine rejects macro sheets
        let Some(cal) = cal.get(&sheet) else { continue };
        let ws = wb.worksheet().unwrap();
        if ws.cell(0, 0).is_err() {
            continue; // MissingReference fixtures
        }
        for at in existing_cells(&input) {
            let c = ws.cell(at.row, at.col).unwrap().unwrap();
            let v = c.value().unwrap_or_else(|e| panic!("{name} {at}: {e}"));
            let d = cal.get(&(at.row, at.col));
            let agree = match (&v, d) {
                (CellValue::Error(_), Some(calamine::Data::Error(_))) => true,
                (CellValue::String(s), None) if s.is_empty() => true,
                _ => calamine_matches(d, &v),
            };
            assert!(agree, "{name} {at}: ours {v:?}, calamine {d:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// Implementation review round 1 (docs/reviews/xlsx-phase0-impl-round1.md)
// ---------------------------------------------------------------------------

const SHEET1: &str = "xl/worksheets/sheet1.xml";
const MC: &str = r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
const AC: &str = "<mc:AlternateContent><mc:Fallback/></mc:AlternateContent>";

fn unresolved(r: &ooxml_xlsx::Result<()>) -> bool {
    matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))))
}

/// F1: a write that is refused changes nothing, even when the refusal comes
/// from the cell's own children. `fixture_minimal` with an
/// `mc:AlternateContent` child added to A2 (value "Alice") and to the
/// value-less D2 (given `t="b"`): before the fix, the setters removed the
/// old value / `t`, then failed, leaving the part dirty and "Alice" lost on
/// save.
#[test]
fn f1_err_inside_cell_changes_nothing() {
    let input = read("minimal");
    let sheet = text(&input, SHEET1)
        .replace(r#"<c r="A2" t="s"><v>1</v></c>"#, &format!(r#"<c r="A2" t="s" {MC}><v>1</v>{AC}</c>"#))
        .replace(r#"<c r="D2" s="1"/>"#, &format!(r#"<c r="D2" s="1" t="b" {MC}>{AC}</c>"#));
    let bytes = rebuild(&input, &[(SHEET1, sheet.into_bytes())]);
    for (cell, w) in [
        ("A2", W::Str("new".into())),
        ("A2", W::Num(1.0)),
        ("D2", W::Num(1.0)),
        ("D2", W::Str("x".into())),
    ] {
        let (wb, r) = apply(&bytes, a1(cell), &w);
        assert!(unresolved(&r), "{cell} {w:?}: {r:?}");
        assert_unchanged(&wb, &bytes);
        // the caller who skips the failed cell and saves keeps the old value
        assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(&format!(r#"<c r="A2" t="s" {MC}><v>1</v>"#)));
    }
    // a neighbour without AC is still writable
    let (_, r) = apply(&bytes, a1("B2"), &W::Num(2.0));
    r.unwrap();
}

/// F1 + F6: an `mc:AlternateContent` child of the row concerned, or of
/// `sheetData` (rows wrapped in AC), may hold the cell: reads are `Err`
/// rather than `Ok(None)`, writes are refused before anything changes.
#[test]
fn f6_alternate_content_around_rows_and_cells() {
    let input = read("minimal");
    // (a) the row-1 element wrapped in AC inside sheetData
    let rows = patched(
        &input,
        SHEET1,
        "<sheetData>\n<row r=\"1\">\n<c r=\"A1\" s=\"1\" t=\"s\"><v>0</v></c>\n</row>",
        &format!(
            "<sheetData {MC}><mc:AlternateContent><mc:Choice Requires=\"x\"/><mc:Fallback><row r=\"1\"><c r=\"A1\" s=\"1\" t=\"s\"><v>0</v></c></row></mc:Fallback></mc:AlternateContent>"
        ),
    );
    // (b) the A2.. cells of row 2 followed by an AC
    let cells =
        patched(&input, SHEET1, "<row r=\"2\">", &format!("<row r=\"2\" {MC}><mc:AlternateContent><mc:Fallback/></mc:AlternateContent>"));
    for (what, sheet) in [("rows in AC", rows), ("AC in row 2", cells)] {
        let bytes = rebuild(&input, &[(SHEET1, sheet)]);
        let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
        {
            let ws = wb.worksheet().unwrap();
            let read = |r, c| ws.cell(r, c).map(|c| c.is_some());
            if what == "rows in AC" {
                assert_matches!(read(0, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{what}");
            }
            assert_matches!(read(1, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{what}");
            assert_matches!(read(1, 7), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{what}");
        }
        assert_unchanged(&wb, &bytes);
        for cell in ["A2", "H2", "B2"] {
            let (wb, r) = apply(&bytes, a1(cell), &W::Num(1.0));
            assert!(unresolved(&r), "{what} {cell}: {r:?}");
            assert_unchanged(&wb, &bytes);
        }
    }
    // (c) a worksheet with an AC and no sheetData: sheetData may be inside it
    let bytes = with_sheet(&format!(r#"{WS_OPEN}<mc:AlternateContent><mc:Choice Requires="mc"><sheetData/></mc:Choice></mc:AlternateContent></worksheet>"#));
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    let before = wb.to_bytes().unwrap();
    let mut ws = wb.worksheet().unwrap();
    assert_matches!(ws.cell(0, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_matches!(ws.set_row_height(0, 18.0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_matches!(ws.cell_mut(0, 0).unwrap().set_number(1.0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), before);
}

/// F7: a `dimension` whose `ref` is missing or not a cell range refuses
/// writes with `Malformed` (it could not be kept up to date); a parseable
/// one is grown (lower case is normalized) or left alone if it already
/// covers the cell.
#[test]
fn f7_dimension_ref() {
    let input = read("minimal");
    let with_dim = |dim: &str| rebuild(&input, &[(SHEET1, patched(&input, SHEET1, r#"<dimension ref="A1:D2"/>"#, dim))]);
    let refused = [
        "A1:D", "A1:D2:E3", "", " A1:D2", "A1:D2 ", "A1 B2", "Sheet1!A1:D2", "A0:D2", "A1:XFD1048577", "A1:XFE1",
    ];
    let refused = refused.iter().map(|d| format!(r#"<dimension ref="{d}"/>"#)).chain(["<dimension/>".to_owned()]);
    for dim in refused {
        let dim = dim.as_str();
        let bytes = with_dim(dim);
        let (wb, r) = apply(&bytes, a1("F9"), &W::Num(1.0));
        assert_matches!(r, Err(Error::Malformed(_)), "{dim}");
        assert_unchanged(&wb, &bytes);
        let (wb, r) = apply(&bytes, a1("B2"), &W::Num(1.0));
        assert_matches!(r, Err(Error::Malformed(_)), "{dim}: refused inside the range too");
        assert_unchanged(&wb, &bytes);
    }
    // accepted spellings (N6): `$` anchors, lower case, either corner order,
    // a single cell; grown to the canonical form when the write is outside
    for (dim, want) in [
        ("a1:d2", "A1:F9"),
        ("$A$1:$D$2", "A1:F9"),
        ("D2:A1", "A1:F9"),
        ("A1:A1", "A1:F9"),
        ("A1", "A1:F9"),
        ("A1:XFD1048576", "A1:XFD1048576"),
    ] {
        let bytes = with_dim(&format!(r#"<dimension ref="{dim}"/>"#));
        let (wb, r) = apply(&bytes, a1("F9"), &W::Num(1.0));
        r.unwrap();
        assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(&format!(r#"<dimension ref="{want}"/>"#)), "{dim}");
    }
    // a write inside the range leaves the spelling byte-for-byte alone (N5)
    for dim in ["a1:d2", "$A$1:$D$2", "D2:A1", "B2", "A1:XFD1048576"] {
        let bytes = with_dim(&format!(r#"<dimension ref="{dim}"/>"#));
        let (wb, r) = apply(&bytes, a1("B2"), &W::Num(1.0));
        r.unwrap();
        assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(&format!(r#"<dimension ref="{dim}"/>"#)), "{dim} inside");
    }
}

/// F4: outer-whitespace trimming follows calamine: only the `t`'s own
/// `xml:space="preserve"` keeps it (an inherited one from `si` is ignored);
/// rich runs are concatenated, phonetic runs skipped. `fixture_minimal`
/// with its shared strings and D2 replaced.
#[test]
fn f4_whitespace_like_calamine() {
    let input = read("minimal");
    let sst = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2"><si><r><t xml:space="preserve">Na</t></r><r><rPr><b/></rPr><t>me </t></r><rPh sb="0" eb="1"><t>ナ</t></rPh><phoneticPr fontId="1"/></si><si xml:space="preserve"><t> Alice </t></si></sst>"#;
    let sheet = patched(
        &input,
        SHEET1,
        r#"<c r="D2" s="1"/>"#,
        r#"<c r="D2" t="inlineStr"><is><t> in </t></is></c><c r="E2" t="inlineStr"><is><t xml:space="preserve"> kept </t></is></c><c r="F2" t="inlineStr" xml:space="preserve"><is><t>	tab </t></is></c>"#,
    );
    let bytes = rebuild(&input, &[("xl/sharedStrings.xml", sst.as_bytes().to_vec()), (SHEET1, sheet)]);
    let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
    let ws = wb.worksheet().unwrap();
    let cal = calamine_cells(&bytes);
    for (r, c, want) in [(0, 0, "Name"), (1, 0, "Alice"), (1, 3, "in"), (1, 4, " kept "), (1, 5, "tab")] {
        let v = ws.cell(r, c).unwrap().unwrap().value().unwrap();
        assert_eq!(v, CellValue::String(want.into()), "({r}, {c})");
        assert!(calamine_matches(cal["Sheet1"].get(&(r, c)), &v), "({r}, {c}): calamine {:?}", cal["Sheet1"].get(&(r, c)));
    }
}
/// F2 / ADR-0001 R2-M3, Phase1 semantics (ADR-0003): a formula only on a
/// sheet other than "sheet1" no longer refuses writes; it is found by the
/// census and loses its cached value when its precedent is written.
/// `poi/51850.xlsx` (no formula, no calcChain) with one row
/// `A9999 = '<first sheet>'!A1*2` appended to sheet2's `sheetData`.
#[test]
fn f2_formula_on_another_sheet_only() {
    let input = read("poi/51850.xlsx");
    assert!(Workbook::from_bytes(input.clone()).unwrap().edit_refusal().is_none(), "the unedited fixture is writable");
    // The formula names the first sheet by its real name (an unknown sheet
    // name would make it depend on everything, ADR-0003 §2.4).
    let first = Workbook::from_bytes(input.clone()).unwrap().sheet_names().next().unwrap().replace('\'', "''");
    let s2 = patched(
        &input,
        "xl/worksheets/sheet2.xml",
        "</sheetData>",
        &format!(r#"<row r="9999"><c r="A9999"><f>'{first}'!A1*2</f><v>0</v></c></row></sheetData>"#),
    );
    let bytes = rebuild(&input, &[("xl/worksheets/sheet2.xml", s2)]);
    let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
    assert_eq!(wb.worksheet_part(), SHEET1);
    assert!(wb.edit_refusal().is_none());
    let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    assert_eq!(wb.dependents(&names[0], a1("A1")).unwrap(), vec![(names[1].clone(), a1("A9999"))]);
    let (wb, r) = apply(&bytes, a1("A1"), &W::Num(1.0));
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/worksheets/sheet2.xml").contains(&format!(r#"<c r="A9999"><f>'{first}'!A1*2</f></c>"#)));
    for at in ["B2", "Z99"] {
        let (wb, r) = apply(&bytes, a1(at), &W::Num(1.0));
        r.unwrap();
        assert_l1_except(&bytes, &wb.to_bytes().unwrap(), &[SHEET1]);
    }
}

/// F2, Phase1 semantics: formulas in macro sheets and dialog sheets count,
/// found through `<sheets>`, the content type or the workbook relationship.
/// A macro-sheet formula that would become a dependent refuses the edit
/// (`MacroSheetDependent`); a sheet part with formulas that `<sheets>` does
/// not list makes the census incomplete (`FormulaScanFailed`).
#[test]
fn f2_formula_in_macro_or_dialog_sheet_only() {
    // (a) poi/xlmmacro.xlsm: its macro sheet (CT override *and* xlMacrosheet
    // relationship) gets one formula cell in its empty sheetData
    let input = read("poi/xlmmacro.xlsm");
    assert!(Workbook::from_bytes(input.clone()).unwrap().edit_refusal().is_none());
    let m = patched(&input, "xl/macrosheets/sheet1.xml", "<sheetData/>", r#"<sheetData><row r="1"><c r="A1"><f>RETURN()</f></c></row></sheetData>"#);
    let bytes = rebuild(&input, &[("xl/macrosheets/sheet1.xml", m)]);
    let wb = Workbook::from_bytes(bytes.clone()).unwrap();
    assert!(wb.edit_refusal().is_none());
    // RETURN() is not on the value-function table: it depends on everything.
    let (wb, r) = apply(&bytes, a1("A1"), &W::Num(1.0));
    assert_matches!(r, Err(Error::MacroSheetDependent { .. }));
    assert_unchanged(&wb, &bytes);

    // `fixture_minimal` plus one extra sheet part holding a formula
    let input = read("minimal");
    let sheet_with_f = |root: &str| {
        format!(r#"<{root} xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><f>1+1</f></c></row></sheetData></{root}>"#)
            .into_bytes()
    };
    // (b) a dialog sheet known only by its content type (no relationship)
    let ct = patched(
        &input,
        "[Content_Types].xml",
        "</Types>",
        r#"<Override PartName="/xl/dialogsheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.dialogsheet+xml"/></Types>"#,
    );
    let bytes = rebuild(&input, &[("[Content_Types].xml", ct), ("xl/dialogsheets/sheet1.xml", sheet_with_f("dialogsheet"))]);
    let wb = Workbook::from_bytes(bytes).unwrap();
    assert_matches!(wb.edit_refusal(), Some(Error::FormulaScanFailed { part, .. }) if part == "xl/dialogsheets/sheet1.xml");
    // (c) a macro sheet known only by its relationship (content type falls
    // back to the `xml` Default, application/xml)
    let rels = patched(
        &input,
        "xl/_rels/workbook.xml.rels",
        "</Relationships>",
        r#"<Relationship Id="rId99" Type="http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet" Target="macrosheets/sheet1.xml"/></Relationships>"#,
    );
    let bytes = rebuild(&input, &[("xl/_rels/workbook.xml.rels", rels), ("xl/macrosheets/sheet1.xml", sheet_with_f("xm:macrosheet"))]);
    let wb = Workbook::from_bytes(bytes).unwrap();
    assert_matches!(wb.edit_refusal(), Some(Error::FormulaScanFailed { part, .. }) if part == "xl/macrosheets/sheet1.xml");
}

/// F2, Phase1 semantics: a `calcChain.xml` alone no longer refuses edits
/// (Phase0's fast path is gone); a value edit with no dependent keeps it
/// byte-identical. `fixture_minimal` plus a calcChain part, its workbook
/// relationship and its content-type override.
#[test]
fn f2_calc_chain_only() {
    let input = read("minimal");
    let rels = patched(
        &input,
        "xl/_rels/workbook.xml.rels",
        "</Relationships>",
        r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#,
    );
    let ct = patched(
        &input,
        "[Content_Types].xml",
        "</Types>",
        r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#,
    );
    let chain = br#"<calcChain xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><c r="B2" i="1"/></calcChain>"#.to_vec();
    let bytes = rebuild(&input, &[("xl/_rels/workbook.xml.rels", rels), ("[Content_Types].xml", ct), ("xl/calcChain.xml", chain)]);
    let wb = Workbook::from_bytes(bytes.clone()).unwrap();
    assert!(wb.edit_refusal().is_none());
    let (wb, r) = apply(&bytes, a1("B2"), &W::Num(1.0));
    r.unwrap();
    assert_l1_except(&bytes, &wb.to_bytes().unwrap(), &[SHEET1]);
}

/// F2: a sheet other than "sheet1" that cannot be scanned (not well-formed;
/// UTF-16) refuses writes with `FormulaScanFailed`; reads of "sheet1" work.
/// `poi/51850.xlsx` with sheet2 replaced.
#[test]
fn f2_unscannable_other_sheet() {
    let input = read("poi/51850.xlsx");
    let broken = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData></worksheet>"#.to_vec();
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(text(&input, "xl/worksheets/sheet2.xml").encode_utf16().flat_map(u16::to_le_bytes));
    for s2 in [broken, utf16] {
        let bytes = rebuild(&input, &[("xl/worksheets/sheet2.xml", s2)]);
        let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
        assert_matches!(wb.edit_refusal(), Some(Error::FormulaScanFailed { part, .. }) if part == "xl/worksheets/sheet2.xml");
        assert!(wb.worksheet().unwrap().cell(0, 0).unwrap().unwrap().value().is_ok());
        let (wb, r) = apply(&bytes, a1("C3"), &W::Str("x".into()));
        assert_matches!(r, Err(Error::FormulaScanFailed { .. }));
        assert_unchanged(&wb, &bytes);
    }
}

/// F5: cells out of column order: reads and writes in that row are refused
/// with `OutOfOrder` (there is no safe insertion point).
#[test]
fn f5_cells_out_of_order() {
    let bytes = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="1"><c r="C1"><v>3</v></c><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row></sheetData></worksheet>"#));
    let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
    {
        let ws = wb.worksheet().unwrap();
        assert_matches!(ws.cell(0, 0), Err(Error::OutOfOrder(_)));
        assert_matches!(ws.cell(0, 2), Err(Error::OutOfOrder(_)));
        assert_eq!(ws.cell(1, 0).unwrap().unwrap().value().unwrap(), CellValue::Number(2.0), "other rows still read");
    }
    for cell in ["A1", "B1", "D1"] {
        let (wb, r) = apply(&bytes, a1(cell), &W::Num(1.0));
        assert_matches!(r, Err(Error::OutOfOrder(_)), "{cell}");
        assert_unchanged(&wb, &bytes);
    }
}

// ---------------------------------------------------------------------------
// Implementation review round 2 (docs/reviews/xlsx-phase0-impl-round2.md)
// ---------------------------------------------------------------------------

/// N1: schema-invalid duplicate `v`/`is` children (around `extLst`, or out
/// of order) are cleaned up, not half-replaced: the write is `Ok`, the cell
/// is left with exactly the new `v` or `is`, and calamine reads the new
/// value with every other cell unchanged (CR; where calamine can read the
/// sheet at all). Before the fix these were
/// `Err` after a mutation (A2 lost) or `Ok` with a stale value calamine
/// still read. `fixture_minimal` with A2 replaced (the reviewer's probe p1).
#[test]
fn n1_duplicate_values_cleaned_up() {
    let input = read("minimal");
    let a2 = r#"<c r="A2" t="s"><v>1</v></c>"#;
    for (cell, writes) in [
        (r#"<c r="A2" t="s"><v>1</v><extLst/><v>1</v></c>"#, [W::Str("new".into()), W::Num(5.0)]),
        (r#"<c r="A2" t="inlineStr"><is><t>a</t></is><extLst/><is><t>b</t></is></c>"#, [W::Num(5.0), W::Str("new".into())]),
        (r#"<c r="A2" t="inlineStr"><is><t>a</t></is><is><t>b</t></is></c>"#, [W::Num(5.0), W::Str(" new".into())]),
        (r#"<c r="A2" t="s"><v>1</v><v>0</v></c>"#, [W::Str("new".into()), W::Num(5.0)]),
        (r#"<c r="A2" t="s"><v>1</v><is><t>zzz</t></is></c>"#, [W::Num(5.0), W::Str("new".into())]),
        (r#"<c r="A2" t="s"><extLst/><v>1</v></c>"#, [W::Str("new".into()), W::Num(5.0)]),
    ] {
        let bytes = rebuild(&input, &[(SHEET1, patched(&input, SHEET1, a2, cell))]);
        for w in writes {
            let (mut wb, r) = apply(&bytes, a1("A2"), &w);
            r.unwrap_or_else(|e| panic!("{cell} {w:?}: {e}"));
            assert_eq!(wb.worksheet().unwrap().cell(1, 0).unwrap().unwrap().value().unwrap(), w.expected());
            let out = wb.to_bytes().unwrap();
            let xml = sheet_xml(&out);
            let c = &xml[xml.find(r#"<c r="A2""#).unwrap()..];
            let c = &c[..c.find("</c>").unwrap()];
            let (v, is) = (c.matches("<v>").count(), c.matches("<is>").count());
            match w {
                W::Num(_) => assert!(v == 1 && is == 0 && !c.contains(" t="), "{cell} {w:?}: {c}"),
                W::Str(_) => assert!(v == 0 && is == 1, "{cell} {w:?}: {c}"),
            }
            if cell.contains("extLst") {
                // calamine 0.38 cannot read a sheet with `extLst` inside `c`
                // (in or out); our re-open must agree with the write
                let mut again = Workbook::from_bytes(out).unwrap();
                assert_eq!(again.worksheet().unwrap().cell(1, 0).unwrap().unwrap().value().unwrap(), w.expected());
            } else {
                assert_cr(&bytes, &out, "Sheet1", a1("A2"), &w.expected());
            }
        }
    }
}

/// N1: refusals that come from the shape around the target (a row whose
/// `extLst` precedes its cells, a worksheet without `sheetData` whose
/// children are out of order, an AC inside the cell) are decided before
/// anything changes; a same-path save after the `Err` keeps the file's
/// entries identical, and after a further successful write keeps every
/// other value.
#[test]
fn n1_refusals_change_nothing_same_path() {
    let input = read("minimal");
    let row1 = "<row r=\"1\">\n<c r=\"A1\"";
    let ext_first = rebuild(&input, &[(SHEET1, patched(&input, SHEET1, row1, "<row r=\"1\"><extLst/>\n<c r=\"A1\""))]);
    let (wb, r) = apply(&ext_first, a1("B1"), &W::Num(1.0));
    assert!(unresolved(&r), "{r:?}");
    assert_unchanged(&wb, &ext_first);
    let ws_order = with_sheet(&format!(r#"{WS_OPEN}<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><dimension ref="A1"/></worksheet>"#));
    let (wb, r) = apply(&ws_order, a1("B2"), &W::Num(1.0));
    assert!(unresolved(&r), "{r:?}");
    assert_unchanged(&wb, &ws_order);

    let dir = temp_dir("n1-same-path");
    let p = dir.join("x.xlsx");
    std::fs::write(&p, &ext_first).unwrap();
    let mut wb = Workbook::open(&p).unwrap();
    assert!(unresolved(&wb.worksheet().unwrap().cell_mut(0, 1).unwrap().set_string_inline("new")));
    wb.save(&p).unwrap();
    assert_l1_except(&ext_first, &std::fs::read(&p).unwrap(), &[]);
    let mut wb = Workbook::open(&p).unwrap();
    assert!(unresolved(&wb.worksheet().unwrap().cell_mut(0, 1).unwrap().set_number(1.0)));
    wb.worksheet().unwrap().cell_mut(1, 1).unwrap().set_number(91.0).unwrap();
    wb.save(&p).unwrap();
    let out = std::fs::read(&p).unwrap();
    assert_cr(&ext_first, &out, "Sheet1", a1("B2"), &CellValue::Number(91.0));
    let _ = std::fs::remove_dir_all(dir);
}

/// N3: an `mc:AlternateContent` inside the cell, or inside its `is`, may
/// hold the value: reads are `Err(UnresolvedSibling)` instead of `Empty` /
/// `""`, and writes are refused with nothing changed. `fixture_minimal`
/// with A2 replaced (the reviewer's probe p7).
#[test]
fn n3_alternate_content_inside_cell_on_read() {
    let input = read("minimal");
    for cell in [
        format!(r#"<c r="A2" t="s" {MC}><mc:AlternateContent><mc:Choice Requires="x"><v>0</v></mc:Choice><mc:Fallback/></mc:AlternateContent></c>"#),
        format!(r#"<c r="A2" t="inlineStr" {MC}><is><mc:AlternateContent><mc:Fallback><t>hidden</t></mc:Fallback></mc:AlternateContent></is></c>"#),
    ] {
        let bytes = rebuild(&input, &[(SHEET1, patched(&input, SHEET1, r#"<c r="A2" t="s"><v>1</v></c>"#, &cell))]);
        let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
        {
            let ws = wb.worksheet().unwrap();
            assert_matches!(ws.cell(1, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{cell}");
            assert!(ws.cell(1, 1).unwrap().is_some(), "neighbours still read");
        }
        assert_unchanged(&wb, &bytes);
        let (wb, r) = apply(&bytes, a1("A2"), &W::Num(1.0));
        assert!(unresolved(&r), "{cell}: {r:?}");
        assert_unchanged(&wb, &bytes);
    }
}

/// N5: only ASCII whitespace is trimmed on read (calamine's rule); NBSP and
/// U+3000 at the edges are text. U+FFFE/U+FFFF are written as `_xHHHH_` and
/// read back (the value's characters are checked before anything changes).
#[test]
fn n5_unicode_edges() {
    let input = read("minimal");
    let sheet = patched(
        &input,
        SHEET1,
        r#"<c r="D2" s="1"/>"#,
        "<c r=\"D2\" t=\"inlineStr\"><is><t>\u{3000}全角\u{3000}</t></is></c><c r=\"E2\" t=\"inlineStr\"><is><t> \u{A0}nb\u{A0} </t></is></c>",
    );
    let bytes = rebuild(&input, &[(SHEET1, sheet)]);
    let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
    let ws = wb.worksheet().unwrap();
    let cal = calamine_cells(&bytes);
    for (c, want) in [(3, "\u{3000}全角\u{3000}"), (4, "\u{A0}nb\u{A0}")] {
        let v = ws.cell(1, c).unwrap().unwrap().value().unwrap();
        assert_eq!(v, CellValue::String(want.into()));
        assert!(calamine_matches(cal["Sheet1"].get(&(1, c)), &v), "calamine {:?}", cal["Sheet1"].get(&(1, c)));
    }
    let w = W::Str("a\u{FFFE}b\u{FFFF}".into());
    let out = check_edit("minimal", &input, a1("C3"), &w);
    assert!(sheet_xml(&out).contains("a_xFFFE_b_xFFFF_"));
}
