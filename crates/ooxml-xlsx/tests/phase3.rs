//! Phase3 acceptance tests (ADR-0009 "Test oracles") on every fixture:
//! rename every sheet, delete each sheet, add at the front and the end,
//! reverse the order, hide each sheet. Every `Ok` must satisfy the
//! oracles; every `Err` must be typed and leave the package unchanged.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use ooxml_core::Package;
use ooxml_core::opc::{TargetMode, resolve_target};
use ooxml_xlsx::{Error, SheetState, Workbook};

/// Internal relationships whose target is missing: (source, id, target).
fn dangling(bytes: &[u8]) -> BTreeSet<(String, String, String)> {
    let pkg = Package::from_bytes(bytes.to_vec()).unwrap();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let lower: BTreeSet<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let mut out = BTreeSet::new();
    let mut sources: Vec<Option<String>> = vec![None];
    sources.extend(names.iter().filter(|n| !n.ends_with(".rels")).cloned().map(Some));
    for s in sources {
        let rels = match &s {
            None => pkg.package_relationships().clone(),
            Some(p) => match pkg.relationships(p) {
                Ok(r) => r,
                Err(_) => continue,
            },
        };
        for r in rels.iter().filter(|r| r.target_mode == TargetMode::Internal && !r.target.trim().starts_with('#')) {
            let t = resolve_target(s.as_deref(), r.target.trim());
            if !lower.contains(&t.to_ascii_lowercase()) {
                out.insert((s.clone().unwrap_or_default(), r.id.clone(), t));
            }
        }
    }
    out
}

/// OPC conformance (oracle 1): no new dangling relationship, every part has
/// a content type, relationship ids unique per part.
fn assert_opc(name: &str, input: &[u8], out: &[u8]) {
    let new: Vec<_> = dangling(out).difference(&dangling(input)).cloned().collect();
    assert!(new.is_empty(), "{name}: new dangling relationships {new:?}");
    let pkg = Package::from_bytes(out.to_vec()).unwrap();
    let old = Package::from_bytes(input.to_vec()).unwrap();
    for p in pkg.part_names() {
        if p != "[Content_Types].xml" && !old.contains(p) {
            assert!(pkg.content_type(p).is_some(), "{name}: {p} has no content type");
        }
        if !p.ends_with(".rels")
            && let Ok(r) = pkg.relationships(p)
        {
            let ids: Vec<String> = r.iter().map(|x| x.id.clone()).collect();
            let set: BTreeSet<&String> = ids.iter().collect();
            assert_eq!(set.len(), ids.len(), "{name}: duplicate ids in {p}'s relationships");
        }
    }
}

/// calamine's formulas, or none when calamine cannot read the file.
fn calamine_formulas(bytes: &[u8]) -> BTreeMap<String, BTreeMap<(u32, u32), String>> {
    std::panic::catch_unwind(|| common::p1::calamine_formulas(bytes)).unwrap_or_default()
}

fn reopen_matches(name: &str, wb: &Workbook, out: &[u8]) {
    let again = Workbook::from_bytes(out.to_vec()).unwrap_or_else(|e| panic!("{name}: the output does not open: {e}"));
    assert_eq!(wb.__index_fingerprint(), again.__index_fingerprint(), "{name}: rebuilt state != reopened state (oracle 1a)");
}

#[derive(Default, Debug)]
struct Tally {
    ok: BTreeMap<&'static str, usize>,
    refused: BTreeMap<String, usize>,
}

fn kind(e: &Error) -> String {
    let s = format!("{e:?}");
    s.split(['(', ' ', '{']).next().unwrap_or_default().to_owned()
}

/// Runs one operation on a fresh workbook; returns the output on success.
fn run(name: &str, input: &[u8], op: &'static str, t: &mut Tally, f: impl FnOnce(&mut Workbook) -> ooxml_xlsx::Result<()>) -> Option<(Workbook, Vec<u8>)> {
    let mut wb = Workbook::from_bytes(input.to_vec()).ok()?;
    if wb.edit_refusal().is_some() {
        return None;
    }
    match f(&mut wb) {
        Ok(()) => {
            *t.ok.entry(op).or_default() += 1;
            let out = wb.to_bytes().unwrap_or_else(|e| panic!("{name} {op}: {e}"));
            assert_opc(&format!("{name} {op}"), input, &out);
            // every changed XML part still parses, every other entry is L1
            let (ei, eo) = (entries(input), entries(&out));
            for e in &eo {
                if ei.iter().any(|x| x == e) {
                    continue;
                }
                let pkg = Package::from_bytes(out.clone()).unwrap();
                let ct = pkg.content_type(&e.0).unwrap_or_default().to_owned();
                if ct.ends_with("xml") || e.0.ends_with(".rels") {
                    ooxml_core::XmlDocument::parse(&pkg.read_part(&e.0).unwrap()).unwrap_or_else(|x| panic!("{name} {op}: {} does not parse: {x}", e.0));
                }
            }
            reopen_matches(&format!("{name} {op}"), &wb, &out);
            Some((wb, out))
        }
        Err(e) => {
            assert!(!matches!(e, Error::Poisoned(_)), "{name} {op}: poisoned: {e}");
            *t.refused.entry(format!("{op} {}", kind(&e))).or_default() += 1;
            assert_l1_except(input, &wb.to_bytes().unwrap(), &[]);
            None
        }
    }
}

fn fixtures() -> Vec<(String, Vec<u8>)> {
    all_zip_fixtures().into_iter().map(|n| (n.clone(), read(&n))).collect()
}

#[test]
fn rename_every_sheet_of_every_fixture() {
    let mut t = Tally::default();
    for (name, input) in fixtures() {
        let Ok(wb) = Workbook::from_bytes(input.clone()) else { continue };
        let sheets: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
        for (i, old) in sheets.iter().enumerate() {
            let new = format!("R{i} it's");
            let label = format!("{name} {old}");
            if let Some((_, out)) = run(&label, &input, "rename", &mut t, |wb| wb.rename_sheet(old, &new).map(drop)) {
                let names: Vec<String> = Workbook::from_bytes(out.clone()).unwrap().sheet_names().map(str::to_owned).collect();
                assert_eq!(names[i], new, "{label}");
                // oracle 4a over every part: no value still names the old
                // sheet (unless another sheet folds alike), and every value
                // that named it (or the new name) now names the new one
                let (vi, vo) = (residual::values(&input), residual::values(&out));
                if !sheets.iter().enumerate().any(|(j, s)| j != i && s.to_lowercase() == old.to_lowercase()) {
                    let left = residual::residual(&vo, old);
                    assert!(left.is_empty(), "{label}: still names {old:?}: {left:#?}");
                    let before = vi.iter().filter(|v| !v.non_reference() && (residual::mentions(&v.text, old) || residual::mentions(&v.text, &new))).count();
                    let after = residual::residual(&vo, &new).len();
                    assert_eq!(after, before, "{label}: {before} values named the sheet, {after} name the new one");
                }
            }
        }
    }
    eprintln!("{t:#?}");
    assert!(t.ok["rename"] >= 30, "{t:#?}");
}

#[test]
fn delete_each_sheet_of_every_fixture() {
    let mut t = Tally::default();
    for (name, input) in fixtures() {
        let Ok(wb) = Workbook::from_bytes(input.clone()) else { continue };
        let sheets: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
        for old in &sheets {
            let label = format!("{name} {old}");
            if let Some((wb2, out)) = run(&label, &input, "delete", &mut t, |wb| wb.delete_sheet(old).map(drop)) {
                assert!(!wb2.sheet_names().any(|s| s == old), "{label}");
                // oracle 4a: the deleted name survives only in locations
                if !sheets.iter().any(|s| s != old && s.to_lowercase() == old.to_lowercase()) {
                    let vo = residual::values(&out);
                    let left: Vec<_> = residual::residual(&vo, old).into_iter().filter(|v| !v.location()).collect();
                    assert!(left.is_empty(), "{label}: still names the deleted sheet: {left:#?}");
                }
                // values of the remaining sheets' constant cells are unchanged (CR)
                let (ci, co) = (try_calamine_cells(&input), try_calamine_cells(&out));
                if let (Some(ci), Some(co)) = (ci, co) {
                    let formulas = calamine_formulas(&input);
                    for (sheet, cells) in &ci {
                        if sheet == old {
                            continue;
                        }
                        let fs = formulas.get(sheet);
                        for (k, v) in cells {
                            if fs.is_some_and(|f| f.contains_key(k)) {
                                continue;
                            }
                            if let Some(o) = co.get(sheet).and_then(|m| m.get(k)) {
                                assert_eq!(o, v, "{label}: {sheet}!{k:?}");
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("{t:#?}");
    assert!(t.ok["delete"] >= 15, "{t:#?}");
}

#[test]
fn add_reverse_and_hide_on_every_fixture() {
    let mut t = Tally::default();
    for (name, input) in fixtures() {
        let Ok(wb) = Workbook::from_bytes(input.clone()) else { continue };
        let n = wb.sheet_names().count();
        run(&name, &input, "add front", &mut t, |wb| wb.add_sheet("New front", 0));
        run(&name, &input, "add end", &mut t, |wb| wb.add_sheet("New end", n));
        run(&name, &input, "reverse", &mut t, |wb| {
            for i in 0..n {
                wb.move_sheet(n - 1, i)?;
            }
            Ok(())
        });
        let sheets: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
        for s in &sheets {
            run(&format!("{name} {s}"), &input, "hide", &mut t, |wb| wb.set_sheet_state(s, SheetState::Hidden));
        }
    }
    eprintln!("{t:#?}");
    assert!(t.ok["add front"] >= 18 && t.ok["add end"] >= 18, "{t:#?}");
}

/// Oracle 1b: a delete that rewrites a dependent's formula and strips its
/// cache (duplicate `v`, two `is`) in one clone.
#[test]
fn strip_and_rewrite_in_one_clone() {
    let sheets = [
        ("Keep", r#"<row r="1"><c r="A1"><f>Gone!A1+1</f><v>2</v><v>3</v></c><c r="B1" t="inlineStr"><f>Gone!B1&amp;"x"</f><is><t>a</t></is><is><t>b</t></is></c></row>"#),
        ("Gone", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
    ];
    let input = common::p1::book(&sheets, "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let rw = wb.delete_sheet("Gone").unwrap();
    assert_eq!(rw.len(), 2, "{rw:?}");
    let out = wb.to_bytes().unwrap();
    let xml = text(&out, "xl/worksheets/sheet1.xml");
    assert!(xml.contains(r#"<c r="A1"><f>#REF!+1</f></c>"#), "{xml}");
    assert!(xml.contains(r#"<c r="B1"><f>#REF!&amp;"x"</f></c>"#), "{xml}");
    assert!(text(&out, "xl/workbook.xml").contains(r#"fullCalcOnLoad="1""#));
    reopen_matches("o1 shape", &wb, &out);
}

/// Defined names, hyperlink locations and `#Sheet!` targets follow a rename;
/// 3-D references and local names follow a delete.
#[test]
fn rename_and_delete_locations() {
    let sheets = [
        ("A", r#"<row r="1"><c r="A1"><f>SUM(A:C!A1)</f></c><c r="B1"><f>'B b'!A1</f></c></row>"#),
        ("B b", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#),
        ("C", r#"<row r="1"><c r="A1"><v>2</v></c></row>"#),
    ];
    let names = r#"<definedName name="G">'B b'!$A$1</definedName><definedName name="L" localSheetId="1">'B b'!$A$1</definedName><definedName name="M" localSheetId="2">C!$A$1</definedName>"#;
    let input = common::p1::book(&sheets, names, "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let rw = wb.rename_sheet("B b", "Bee").unwrap();
    assert_eq!(rw.len(), 3, "{rw:?}");
    let out = wb.to_bytes().unwrap();
    let w = text(&out, "xl/workbook.xml");
    assert!(w.contains(r#"<definedName name="G">Bee!$A$1</definedName>"#), "{w}");
    assert!(text(&out, "xl/worksheets/sheet1.xml").contains("<f>Bee!A1</f>"));
    // delete the middle sheet: the 3-D span is unchanged, the local name goes,
    // the later local name is renumbered
    let rw = wb.delete_sheet("Bee").unwrap();
    let out = wb.to_bytes().unwrap();
    let w = text(&out, "xl/workbook.xml");
    assert!(!w.contains(r#"name="L""#), "{w}");
    assert!(w.contains(r#"<definedName name="M" localSheetId="1">C!$A$1</definedName>"#), "{w}");
    assert!(w.contains(r#"<definedName name="G">#REF!</definedName>"#), "{w}");
    let s1 = text(&out, "xl/worksheets/sheet1.xml");
    assert!(s1.contains("<f>SUM(A:C!A1)</f>") && s1.contains("<f>#REF!</f>"), "{s1} {rw:?}");
    // deleting an endpoint moves it inward
    wb.delete_sheet("C").unwrap();
    let s1 = text(&wb.to_bytes().unwrap(), "xl/worksheets/sheet1.xml");
    assert!(s1.contains("<f>SUM(A!A1)</f>"), "{s1}");
}

/// Moves that reverse a 3-D reference are refused; others pass.
#[test]
fn move_respects_three_d_order() {
    let sheets = [("A", r#"<row r="1"><c r="A1"><f>SUM(B:C!A1)</f></c></row>"#), ("B", ""), ("C", "")];
    let input = common::p1::book(&sheets, "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(matches!(wb.move_sheet(2, 1), Err(Error::ReversesThreeDReference(_))));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    wb.move_sheet(0, 2).unwrap();
    assert_eq!(wb.sheet_names().collect::<Vec<_>>(), ["B", "C", "A"]);
}

/// A shape hyperlink's `#Sheet!` target follows a rename; a new name with a
/// control character is refused (ADR-0009 R3-m4).
#[test]
fn hyperlink_targets_follow_a_rename() {
    let sheets = [("Sheet1", ""), ("Data", "")];
    let rels = r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#Data!A1"/></Relationships>"##;
    let sheet1 = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><hyperlinks><hyperlink ref="A1" location="Data!B2"/></hyperlinks></worksheet>"#;
    let input = rebuild(&common::p1::book(&sheets, "", ""), &[("xl/worksheets/sheet1.xml", sheet1.as_bytes().to_vec()), ("xl/worksheets/_rels/sheet1.xml.rels", rels.as_bytes().to_vec())]);
    // a control character in the new name cannot go into the Target
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(matches!(wb.rename_sheet("Data", "Da\nta"), Err(Error::UnrewritableReference { .. }) | Err(Error::InvalidSheetName { .. })));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let rw = wb.rename_sheet("Data", "My Data").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/worksheets/sheet1.xml").contains(r#"location="'My Data'!B2""#), "{rw:?}");
    assert!(text(&out, "xl/worksheets/_rels/sheet1.xml.rels").contains(r##"Target="#'My Data'!A1""##), "{rw:?}");
    assert_eq!(rw.len(), 2, "{rw:?}");
}

/// Oracle 1c: a failure or panic between the start of apply and the end
/// of the rebuild leaves the workbook poisoned.
#[cfg(feature = "fault-injection")]
#[test]
fn structural_failures_poison() {
    use ooxml_xlsx::ApplyStep;
    for step in [ApplyStep::StructuralApply, ApplyStep::StructuralRebuild, ApplyStep::StructuralReindex] {
        for panic in [false, true] {
            let mut wb = Workbook::new().unwrap();
            if panic {
                wb.inject_apply_panic(step);
            } else {
                wb.inject_apply_failure(step);
            }
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wb.add_sheet("X", 1)));
            if let Ok(r) = r {
                assert!(matches!(r, Err(Error::Poisoned(_))), "{step:?}: {r:?}");
            }
            assert!(matches!(wb.edit_refusal(), Some(Error::Poisoned(_))), "{step:?} panic={panic}");
            assert!(matches!(wb.to_bytes(), Err(Error::Poisoned(_))));
        }
    }
}

/// Chart series formulas (`c:f`) follow a rename; only their text changes.
#[test]
fn chart_formulas_follow_a_rename() {
    let input = read("poi/57362.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let rw = wb.rename_sheet("Sheet1", "Daten 1").unwrap();
    assert!(rw.iter().filter(|r| r.location == "c:f").count() >= 4, "{rw:?}");
    let out = wb.to_bytes().unwrap();
    let (before, after) = (text(&input, "xl/charts/chart1.xml"), text(&out, "xl/charts/chart1.xml"));
    assert!(!after.contains("Sheet1!") && after.contains("<c:f>'Daten 1'!$A$1:$B$1</c:f>"));
    assert_eq!(before.replace("Sheet1!", "'Daten 1'!"), after, "byte-local: only the c:f text changes");
}

const WS_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn sheet_xml_with(inner: &str) -> Vec<u8> {
    format!(r#"<worksheet xmlns="{WS_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{inner}</worksheet>"#).into_bytes()
}

/// A part another sheet also uses stays when its sheet is deleted (closed
/// set: shared parts are kept).
#[test]
fn delete_keeps_shared_parts() {
    let rels = format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/printerSettings" Target="../printerSettings/p1.bin"/></Relationships>"#);
    let base = common::p1::book(&[("A", ""), ("B", "")], "", "");
    let input = rebuild(&base, &[
        ("xl/worksheets/_rels/sheet1.xml.rels", rels.clone().into_bytes()),
        ("xl/worksheets/_rels/sheet2.xml.rels", rels.into_bytes()),
        ("xl/printerSettings/p1.bin", b"PS".to_vec()),
    ]);
    let input = common::p1::with_part(&input, "[Content_Types].xml", &text(&input, "[Content_Types].xml").replace("</Types>", r#"<Default Extension="bin" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings"/></Types>"#));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.delete_sheet("A").unwrap();
    let pkg = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    assert!(pkg.contains("xl/printerSettings/p1.bin"), "the shared part stays");
    assert!(!pkg.contains("xl/worksheets/_rels/sheet1.xml.rels"));
}

/// A deleted sheet's id is not reused by a later add (ADR-0009 §3).
#[test]
fn sheet_ids_are_not_reused() {
    let mut wb = Workbook::from_bytes(common::p1::book(&[("A", ""), ("B", "")], "", "")).unwrap();
    wb.delete_sheet("B").unwrap();
    wb.add_sheet("C", 1).unwrap();
    let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(w.contains(r#"name="C" sheetId="3""#), "{w}");
}

/// The closed-world scan refuses a sheet mention in an unknown XML part.
#[test]
fn scan_refuses_unknown_places() {
    let base = common::p1::book(&[("A", ""), ("Data", "")], "", "");
    let input = rebuild(&base, &[("xl/custom.xml", br#"<x a="Data!A1"/>"#.to_vec())]);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(matches!(wb.rename_sheet("Data", "D2"), Err(Error::UnrewritableReference { .. })));
    assert!(wb.to_bytes().unwrap() == input);
    assert!(matches!(wb.delete_sheet("Data"), Err(Error::UnrewritableReference { .. })));
    assert!(wb.to_bytes().unwrap() == input);
}

/// Deleting the active, selected sheet selects the next visible one, and
/// `activeTab` follows positions.
#[test]
fn delete_moves_the_active_tab() {
    let views = r#"<bookViews><workbookView activeTab="2"/></bookViews>"#;
    let base = common::p1::book(&[("A", ""), ("B", ""), ("C", ""), ("D", "")], "", views);
    let selected = sheet_xml_with(r#"<sheetViews><sheetView tabSelected="1" workbookViewId="0"/></sheetViews><sheetData/>"#);
    let plain = sheet_xml_with(r#"<sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetData/>"#);
    let input = rebuild(&base, &[("xl/worksheets/sheet3.xml", selected), ("xl/worksheets/sheet2.xml", plain.clone()), ("xl/worksheets/sheet4.xml", plain)]);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.delete_sheet("A").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/workbook.xml").contains(r#"activeTab="1""#), "C moved from 2 to 1");
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.delete_sheet("C").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/workbook.xml").contains(r#"activeTab="2""#), "D (now at 2) is active");
    assert!(text(&out, "xl/worksheets/sheet4.xml").contains(r#"tabSelected="1""#), "D is selected now");
}

/// A table used elsewhere refuses the delete of its sheet.
#[test]
fn table_in_use_refuses_delete() {
    let base = common::p1::book(&[("A", r#"<row r="1"><c r="A1"><f>SUM(Tbl[Col])</f></c></row>"#), ("B", "")], "", "");
    let rels = format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#);
    let table = format!(r#"<table xmlns="{WS_NS}" id="1" name="Tbl" displayName="Tbl" ref="A1:A2"><tableColumns count="1"><tableColumn id="1" name="Col"/></tableColumns></table>"#);
    let input = rebuild(&base, &[
        ("xl/worksheets/sheet2.xml", sheet_xml_with(r#"<sheetData/><tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#)),
        ("xl/worksheets/_rels/sheet2.xml.rels", rels.into_bytes()),
        ("xl/tables/table1.xml", table.into_bytes()),
    ]);
    let input = common::p1::with_part(&input, "[Content_Types].xml", &text(&input, "[Content_Types].xml").replace("</Types>", r#"<Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/></Types>"#));
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(matches!(wb.delete_sheet("B"), Err(Error::TableInUse(_))));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

/// A pivot cache sourcing the sheet refuses its delete (ADR-0009 §5).
#[test]
fn pivot_source_refuses_delete() {
    let input = read("poi/ExcelPivotTableSample.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(matches!(wb.delete_sheet("Tabelle1"), Err(Error::SheetIsPivotSource(_))));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

/// `localSheetId`s at or after an added sheet's position shift by one.
#[test]
fn add_shifts_local_names() {
    let names = r#"<definedName name="L0" localSheetId="0">A!$A$1</definedName><definedName name="L1" localSheetId="1">B!$A$1</definedName>"#;
    let mut wb = Workbook::from_bytes(common::p1::book(&[("A", ""), ("B", "")], names, "")).unwrap();
    wb.add_sheet("New", 1).unwrap();
    let w = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(w.contains(r#"name="L0" localSheetId="0""#) && w.contains(r#"name="L1" localSheetId="2""#), "{w}");
}

/// Shared workbooks refuse every structural operation.
#[test]
fn shared_workbooks_refuse() {
    let base = common::p1::book(&[("A", ""), ("B", "")], "", "");
    let rels = text(&base, "xl/_rels/workbook.xml.rels").replace(
        "</Relationships>",
        r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/revisionHeaders" Target="revisions/revisionHeaders.xml"/></Relationships>"#,
    );
    let input = rebuild(&base, &[("xl/_rels/workbook.xml.rels", rels.into_bytes()), ("xl/revisions/revisionHeaders.xml", b"<headers/>".to_vec())]);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(matches!(wb.add_sheet("C", 0), Err(Error::SharedWorkbook)));
    assert!(matches!(wb.rename_sheet("A", "Z"), Err(Error::SharedWorkbook)));
    assert!(matches!(wb.delete_sheet("B"), Err(Error::SharedWorkbook)));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

/// A delete removes `calcChain.xml` (the set of formula cells changes).
#[test]
fn delete_removes_calc_chain() {
    let base = common::p1::book(&[("A", r#"<row r="1"><c r="A1"><f>1+1</f><v>2</v></c></row>"#), ("B", r#"<row r="1"><c r="A1"><f>2+2</f><v>4</v></c></row>"#)], "", "");
    let rels = text(&base, "xl/_rels/workbook.xml.rels").replace(
        "</Relationships>",
        r#"<Relationship Id="rId98" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#,
    );
    let cc = format!(r#"<calcChain xmlns="{WS_NS}"><c r="A1" i="1"/><c r="A1" i="2"/></calcChain>"#);
    let input = rebuild(&base, &[("xl/_rels/workbook.xml.rels", rels.into_bytes()), ("xl/calcChain.xml", cc.into_bytes())]);
    let input = common::p1::with_part(&input, "[Content_Types].xml", &text(&input, "[Content_Types].xml").replace("</Types>", r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.delete_sheet("B").unwrap();
    let pkg = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    assert!(!pkg.contains("xl/calcChain.xml"));
    assert!(!text(&wb.to_bytes().unwrap(), "[Content_Types].xml").contains("calcChain"));
}
