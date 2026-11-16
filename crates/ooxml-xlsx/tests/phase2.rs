//! Phase2 acceptance tests (ADR-0006 "Test oracles"): the prototype's
//! checks (a)–(d) ported over every fixture, growth bounds, exact-form
//! reuse across sessions, refusals, dates, style-only invalidation.

mod common;

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use common::*;
use ooxml_core::{QName, RawNode, XmlDocument};
use ooxml_xlsx::{Color, Error, ExcelDateTime, Fill, Format, SML_NS, Workbook};

const NEW_TEXT: &str = "ooxml-rs Phase2 string";
const NEW_NUMBER: f64 = 1234.5678;
const NEW_CODE: &str = "0.000\" p2\"";

fn x(local: &'static str) -> QName {
    QName::new(SML_NS, local)
}

fn phase2_fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{ROOT}/fixtures/xlsx/poi-phase2/{name}")).unwrap()
}

fn full_format() -> Format {
    Format::new()
        .set_num_format(NEW_CODE)
        .set_bold()
        .set_font_color(Color::rgb(0xFF0000))
        .set_background_color(Color::rgb(0xFFFF00))
        .set_border("thin")
}

fn parse(bytes: &[u8]) -> XmlDocument {
    XmlDocument::parse(bytes).unwrap()
}

/// The part a workbook relationship of this type points at.
fn rel_target(bytes: &[u8], wb_part: &str, ty: &str) -> Option<String> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    let rels = pkg.relationships(wb_part).unwrap();
    rels.by_type(ty).next().and_then(|r| rels.resolve(r)).filter(|p| pkg.contains(p))
}

/// Replaces `from` (once) in a part of the package.
fn patch_part(bytes: &[u8], name: &str, from: &str, to: &str) -> Vec<u8> {
    p1::with_part(bytes, name, &String::from_utf8(patched(bytes, name, from, to)).unwrap())
}

/// The workbook part, through the package's `officeDocument` relationship
/// (content types alone mislead: `65016.xlsx` gives every `.xml` part the
/// main content type by `Default`, templates use `template.main+xml`).
fn workbook_part(bytes: &[u8]) -> String {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    pkg.main_document_part().expect("officeDocument relationship")
}

const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_SST: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";

fn table<'d>(root: &'d RawNode, name: &str) -> Option<&'d RawNode> {
    root.elements().find(|e| e.name().ns() == SML_NS && e.name().local() == name)
}

fn src<'s>(doc: &'s XmlDocument, n: &RawNode) -> &'s str {
    let s = n.span().unwrap();
    &doc.source()[s.start..s.end()]
}

/// Entry bytes per style table.
fn style_entries(doc: &XmlDocument) -> BTreeMap<&'static str, Vec<String>> {
    let mut m = BTreeMap::new();
    for t in ["numFmts", "fonts", "fills", "borders", "cellStyleXfs", "cellXfs", "cellStyles", "dxfs"] {
        let v = table(doc.root(), t).map(|n| n.elements().map(|e| src(doc, e).to_owned()).collect()).unwrap_or_default();
        m.insert(t, v);
    }
    m
}

fn sst_entries(doc: &XmlDocument) -> Vec<String> {
    doc.root().elements().map(|e| src(doc, e).to_owned()).collect()
}

/// Every element in document order.
fn walk<'n>(n: &'n RawNode, out: &mut Vec<&'n RawNode>) {
    out.push(n);
    for c in n.elements() {
        walk(c, out);
    }
}

/// (d): `t="s"` cells counted independently of the writer: every part whose
/// content type is a worksheet, macro sheet or dialog sheet (found without
/// relationships).
fn count_s_refs_by_content_type(bytes: &[u8]) -> u64 {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    let mut n = 0;
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    for name in names {
        let ct = pkg.content_type(&name).unwrap_or_default();
        if !(ct.ends_with("worksheet+xml") || ct.ends_with("macrosheet+xml") || ct.ends_with("dialogsheet+xml")) {
            continue;
        }
        let doc = parse(&pkg.read_part(&name).unwrap());
        let mut all = Vec::new();
        walk(doc.root(), &mut all);
        n += all.iter().filter(|e| e.is(x("c")) && e.attr(QName::new("", "t")) == Some("s")).count() as u64;
    }
    n
}

/// T2-append (ADR-0006 oracle 2): bytes differ only in the open tags of the
/// tables, the appended entries (at the original close tag, or before
/// `extLst`), inserted tables and re-rendered self-closing tables.
fn check_locality(a: &XmlDocument, b: &XmlDocument, tables: &[(&'static str, usize)], root_is_table: Option<usize>) -> Result<(), String> {
    let (ar, br) = (a.root(), b.root());
    let mut regions: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    if ar.span().unwrap().is_self_closing() {
        let (sa, sb) = (ar.span().unwrap(), br.span().unwrap());
        return if a.source()[..sa.start] == b.source()[..sb.start] && a.source()[sa.end()..] == b.source()[sb.end()..] {
            Ok(())
        } else {
            Err("bytes differ outside the re-rendered self-closing root".into())
        };
    }
    let one = |at: Option<&RawNode>, bt: &RawNode, n_in: usize, regions: &mut Vec<_>| -> Result<(), String> {
        let bs = bt.span().unwrap();
        let Some(at) = at else {
            let next = br.elements().skip_while(|n| !std::ptr::eq(*n, bt)).nth(1).ok_or("inserted table is last")?;
            let a_next = ar.elements().find(|n| n.name().local() == next.name().local()).ok_or("next sibling not in input")?;
            let p = a_next.span().unwrap().start;
            regions.push((p..p, bs.start..bs.end()));
            return Ok(());
        };
        let as_ = at.span().unwrap();
        if as_.is_self_closing() {
            regions.push((as_.start..as_.end(), bs.start..bs.end()));
            return Ok(());
        }
        regions.push((as_.start..as_.open_end, bs.start..bs.open_end));
        let be: Vec<_> = bt.elements().collect();
        let new = &be[n_in..];
        let (Some(first), Some(last)) = (new.first(), new.last()) else { return Ok(()) };
        let p = at.elements().find(|n| n.is(x("extLst"))).map_or(as_.close.unwrap().0, |n| n.span().unwrap().start);
        regions.push((p..p, first.span().unwrap().start..last.span().unwrap().end()));
        Ok(())
    };
    if let Some(n_in) = root_is_table {
        one(Some(ar), br, n_in, &mut regions)?;
    }
    for &(name, n_in) in tables {
        let Some(bt) = table(br, name) else { continue };
        one(table(ar, name), bt, n_in, &mut regions)?;
    }
    // an insertion point sorts before a region starting at the same offset
    regions.sort_by_key(|r| (r.0.start, r.0.end));
    let (asrc, bsrc) = (a.source(), b.source());
    let (mut pa, mut pb) = (0, 0);
    for (ra, rb) in &regions {
        if asrc.get(pa..ra.start) != bsrc.get(pb..rb.start) {
            return Err(format!("bytes differ outside allowed regions before input offset {}", ra.start));
        }
        (pa, pb) = (ra.end, rb.end);
    }
    if asrc[pa..] != bsrc[pb..] {
        return Err("bytes differ after the last allowed region".into());
    }
    Ok(())
}

/// The default style's font (`cellStyleXfs[0]`, else `cellXfs[0]`): name, size.
fn default_font(doc: &XmlDocument) -> Option<(String, f64)> {
    let root = doc.root();
    let xf = table(root, "cellStyleXfs").and_then(|t| t.elements().next()).or_else(|| table(root, "cellXfs").and_then(|t| t.elements().next()))?;
    let id: usize = xf.attr(QName::new("", "fontId"))?.parse().ok()?;
    let font = table(root, "fonts")?.elements().nth(id)?;
    let val = |l: &'static str| font.elements().find(|e| e.is(x(l))).and_then(|e| e.attr(QName::new("", "val")).map(str::to_owned));
    Some((val("name")?, val("sz")?.parse().ok()?))
}

fn max_row(sheet: &[u8]) -> Option<u32> {
    let doc = parse(sheet);
    let sd = table(doc.root(), "sheetData")?;
    Some(sd.elements().filter_map(|r| r.attr(QName::new("", "r")).and_then(|v| v.parse().ok())).max().unwrap_or(0))
}

/// `s` of every pre-existing cell.
fn cell_styles(sheet: &[u8]) -> BTreeMap<String, Option<String>> {
    let doc = parse(sheet);
    let mut all = Vec::new();
    walk(doc.root(), &mut all);
    all.iter()
        .filter(|e| e.is(x("c")))
        .filter_map(|e| Some((e.attr(QName::new("", "r"))?.to_owned(), e.attr(QName::new("", "s")).map(str::to_owned))))
        .collect()
}

#[derive(Default, Debug)]
struct Tally {
    ok: usize,
    refused: Vec<String>,
    strings_appended: usize,
    styles_appended: usize,
}

/// Runs `f` and, if it fails, checks that the workbook still saves to the
/// same bytes as before (refusals change nothing, oracle 8).
fn guarded(wb: &mut Workbook, f: impl FnOnce(&mut Workbook) -> ooxml_xlsx::Result<()>) -> ooxml_xlsx::Result<()> {
    let before = wb.to_bytes().ok();
    let r = f(wb);
    if r.is_err()
        && let Some(before) = before
    {
        assert_l1_except(&before, &wb.to_bytes().unwrap(), &[]);
    }
    r
}

/// One package, edit mode: the prototype's run, through the crate's API.
fn run_one(name: &str, input: &[u8], t: &mut Tally) {
    let Ok(mut wb) = Workbook::from_bytes(input.to_vec()) else {
        t.refused.push(format!("{name}: does not open"));
        return;
    };
    if wb.edit_refusal().is_some() {
        t.refused.push(format!("{name}: edits refused workbook-wide"));
        return;
    }
    let wb_part = workbook_part(input);
    let sheet_part = wb.worksheet_part().to_owned();
    let sheet_name = wb.worksheet().unwrap().name().to_owned();
    let Some(row) = max_row(&part(input, &sheet_part)) else {
        t.refused.push(format!("{name}: no sheetData"));
        return;
    };
    let styles_part = rel_target(input, &wb_part, REL_STYLES);
    let sst_part = rel_target(input, &wb_part, REL_SST);

    let a = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(row, 0)?.set_string(NEW_TEXT));
    let b = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(row, 1)?.set_number_with_format(NEW_NUMBER, &full_format()));
    let c = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(row, 2)?.set_number_with_format(NEW_NUMBER, &Format::new().set_num_format(NEW_CODE)));
    if let Err(e) = &a {
        t.refused.push(format!("{name}: string {e}"));
        return;
    }
    if let Err(e) = &b {
        t.refused.push(format!("{name}: format {e}"));
    }
    assert_eq!(b.is_ok(), c.is_ok(), "{name}: B and C succeed or fail together ({b:?} / {c:?})");
    let out = wb.to_bytes().unwrap();

    // L1 for every other part. A write with formula dependents (Phase1)
    // also strips their caches and sets fullCalcOnLoad.
    // In manual calculation mode Phase1 strips dependents' caches without
    // setting fullCalcOnLoad (ADR-0003), so workbook.xml does not tell.
    let manual = parse(&part(input, &wb_part))
        .root()
        .elements()
        .find(|e| e.is(x("calcPr")))
        .and_then(|c| c.attr(QName::new("", "calcMode")))
        .is_some_and(|m| m.trim() == "manual");
    let invalidated = manual || part(input, &wb_part) != part(&out, &wb_part);
    let sheet_parts: Vec<String> = p1::sheet_parts(input).into_values().collect();
    let mut changed = vec![sheet_part.as_str()];
    if invalidated {
        changed.push(&wb_part);
        changed.extend(sheet_parts.iter().map(String::as_str));
    }
    if let Some(s) = &styles_part {
        changed.push(s);
    }
    if let Some(s) = &sst_part {
        changed.push(s);
    }
    assert_l1_except(input, &out, &changed);

    // (a) + (b) styles
    if let Some(sp) = &styles_part {
        let (si, so) = (parse(&part(input, sp)), parse(&part(&out, sp)));
        let (ei, eo) = (style_entries(&si), style_entries(&so));
        for (t_name, v) in &ei {
            assert!(eo[t_name].len() >= v.len(), "{name}: {t_name} shrank");
            assert_eq!(&eo[t_name][..v.len()], &v[..], "{name}: (a) {t_name} entries changed");
        }
        let grown: Vec<(&'static str, usize)> = ei.iter().map(|(k, v)| (*k, v.len())).collect();
        if b.is_ok() {
            if eo["cellXfs"].len() > ei["cellXfs"].len() {
                t.styles_appended += 1;
            }
            check_locality(&si, &so, &grown, None).unwrap_or_else(|e| panic!("{name}: (b) styles.xml: {e}"));
        } else {
            assert_eq!(part(input, sp), part(&out, sp), "{name}: a refused format leaves styles.xml");
        }
    }
    // (a) + (b) + (d) shared strings
    if let Some(sp) = &sst_part {
        let (si, so) = (parse(&part(input, sp)), parse(&part(&out, sp)));
        let (ei, eo) = (sst_entries(&si), sst_entries(&so));
        assert_eq!(&eo[..ei.len()], &ei[..], "{name}: (a) si entries changed");
        assert!(eo.len() <= ei.len() + 1, "{name}: at most one si appended");
        if eo.len() > ei.len() {
            t.strings_appended += 1;
        }
        check_locality(&si, &so, &[], Some(ei.len())).unwrap_or_else(|e| panic!("{name}: (b) sst: {e}"));
        let root = so.root();
        if let Some(u) = root.attr(QName::new("", "uniqueCount")) {
            assert_eq!(u.parse::<usize>().unwrap(), eo.len(), "{name}: (d) uniqueCount");
        }
        if let Some(c) = root.attr(QName::new("", "count")) {
            assert_eq!(c.parse::<u64>().unwrap(), count_s_refs_by_content_type(&out), "{name}: (d) count");
        }
        assert_eq!(root.attr(QName::new("", "count")).is_some(), si.root().attr(QName::new("", "count")).is_some(), "{name}: absent count stays absent");
    }
    // (a) pre-existing cells keep `s`
    let (ci, co) = (cell_styles(&part(input, &sheet_part)), cell_styles(&part(&out, &sheet_part)));
    for (r, s) in &ci {
        assert_eq!(co.get(r), Some(s), "{name}: (a) cell {r} s changed");
    }

    // (c) calamine
    if let (Some(mut cin), Some(mut cout)) = (try_calamine_cells(input), try_calamine_cells(&out))
        && cin.contains_key(&sheet_name)
    {
        let m = cout.get_mut(&sheet_name).expect("sheet");
        assert_eq!(m.remove(&(row, 0)), Some(Data::String(NEW_TEXT.into())), "{name}: (c) A");
        if b.is_ok() {
            assert_eq!(m.remove(&(row, 1)), Some(Data::Float(NEW_NUMBER)), "{name}: (c) B");
            assert_eq!(m.remove(&(row, 2)), Some(Data::Float(NEW_NUMBER)), "{name}: (c) C");
        }
        if let Some(mi) = cin.get_mut(&sheet_name) {
            for c in 0..3 {
                mi.remove(&(row, c));
            }
        }
        if invalidated {
            // dependents lost their cached values: compare constants only
            for (sheet, fs) in p1::calamine_formulas(input) {
                for k in fs.keys() {
                    cin.get_mut(&sheet).map(|m| m.remove(k));
                    cout.get_mut(&sheet).map(|m| m.remove(k));
                }
            }
            // array result cells lose their values too: every value that is
            // still there must be unchanged
            for (sheet, m) in cin.iter_mut() {
                let o = cout.get(sheet);
                m.retain(|k, _| o.is_some_and(|o| o.contains_key(k)));
            }
        }
        if cin != cout {
            let mut diffs = Vec::new();
            for (sheet, mi) in &cin {
                let mo = cout.get(sheet);
                for (k, v) in mi {
                    if mo.and_then(|m| m.get(k)) != Some(v) {
                        diffs.push(format!("{sheet}!{k:?}: {v:?} -> {:?}", mo.and_then(|m| m.get(k))));
                    }
                }
                for (k, v) in mo.into_iter().flatten() {
                    if !mi.contains_key(k) {
                        diffs.push(format!("{sheet}!{k:?}: (none) -> {v:?}"));
                    }
                }
            }
            panic!("{name}: (c) CR: other cells differ: {}", diffs.into_iter().take(8).collect::<Vec<_>>().join("; "));
        }
        if b.is_ok() {
            let mut xw: Xlsx<_> = Xlsx::new(Cursor::new(out.clone())).unwrap();
            let styles = xw.worksheet_style(&sheet_name).unwrap();
            let sb = styles.get((row, 1)).expect("style at B");
            assert_eq!(sb.number_format.as_ref().map(|n| n.format_code.as_str()), Some(NEW_CODE), "{name}: (c) numFmt");
            assert!(sb.font.as_ref().is_some_and(|f| f.is_bold()), "{name}: (c) bold");
            let fg = sb.fill.as_ref().and_then(|f| f.get_color()).map(|c| c.to_argb() & 0xFF_FFFF);
            assert_eq!(fg, Some(0xFFFF00), "{name}: (c) fill");
            let sc = styles.get((row, 2)).expect("style at C");
            assert_eq!(sc.number_format.as_ref().map(|n| n.format_code.as_str()), Some(NEW_CODE), "{name}: (c) C numFmt");
            if let Some(sp) = &styles_part
                && let Some((fname, fsz)) = default_font(&parse(&part(input, sp)))
            {
                let f = sc.font.as_ref().expect("font at C");
                assert_eq!((f.name.as_deref(), f.size), (Some(fname.as_str()), Some(fsz)), "{name}: (M1) C inherits the default font");
            }
        }
    }

    // session 2 (M2): the same writes again add nothing
    if b.is_ok() {
        let mut wb2 = Workbook::from_bytes(out.clone()).unwrap();
        {
            let mut ws = wb2.worksheet().unwrap();
            ws.cell_mut(row + 1, 0).unwrap().set_string(NEW_TEXT).unwrap();
            ws.cell_mut(row + 1, 1).unwrap().set_number_with_format(NEW_NUMBER, &full_format()).unwrap();
            ws.cell_mut(row + 1, 2).unwrap().set_number_with_format(NEW_NUMBER, &Format::new().set_num_format(NEW_CODE)).unwrap();
        }
        let out2 = wb2.to_bytes().unwrap();
        if let Some(sp) = &styles_part {
            assert_eq!(style_entries(&parse(&part(&out, sp))), style_entries(&parse(&part(&out2, sp))), "{name}: (M2) styles grew in session 2");
        }
        if let Some(sp) = &sst_part {
            assert_eq!(sst_entries(&parse(&part(&out, sp))), sst_entries(&parse(&part(&out2, sp))), "{name}: (M2) sst grew in session 2");
            let c2 = parse(&part(&out2, sp));
            if let Some(c) = c2.root().attr(QName::new("", "count")) {
                assert_eq!(c.parse::<u64>().unwrap(), count_s_refs_by_content_type(&out2), "{name}: (d) count in session 2");
            }
        }
    }
    t.ok += 1;
}

#[test]
fn registry_sweep_over_every_fixture() {
    let mut t = Tally::default();
    let mut names = all_zip_fixtures();
    names.extend(["47889.xlsx", "64420.xlsm", "64450.xlsx"].map(|n| format!("phase2:{n}")));
    for name in &names {
        let input = match name.strip_prefix("phase2:") {
            Some(n) => phase2_fixture(n),
            None => read(name),
        };
        run_one(name, &input, &mut t);
    }
    eprintln!("{t:#?}");
    assert!(t.ok >= 18, "{t:#?}");
    assert!(t.strings_appended >= 12 && t.styles_appended >= 16, "{t:#?}");
}

/// The same per-package checks over a local corpus directory (the POI
/// `test-data/spreadsheet` download of `spikes/xlsx-phase2/README.md`). Not
/// in CI: the files are not committed. Every package that opens is run;
/// failures are collected and reported together.
#[test]
#[ignore = "needs OOXML_XLSX_CORPUS=<directory of .xlsx/.xlsm files>"]
fn registry_sweep_over_a_local_corpus() {
    let dir = std::env::var("OOXML_XLSX_CORPUS").expect("set OOXML_XLSX_CORPUS");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xlsx") || x.eq_ignore_ascii_case("xlsm")))
        .collect();
    files.sort();
    let mut t = Tally::default();
    let mut failed = Vec::new();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let input = std::fs::read(f).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_one(&name, &input, &mut t)));
        if let Err(p) = r {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
            failed.push(format!("{name}: {}", msg.lines().next().unwrap_or_default()));
        }
    }
    std::panic::set_hook(hook);
    eprintln!("{} files; ok {}; strings appended {}; styles appended {}; refused {}", files.len(), t.ok, t.strings_appended, t.styles_appended, t.refused.len());
    for r in &t.refused {
        eprintln!("  refused: {r}");
    }
    assert!(failed.is_empty(), "{} packages failed:\n{}", failed.len(), failed.join("\n"));
}

// ------------------------------------------------------------ growth bound (oracle 6)

fn counts(bytes: &[u8]) -> BTreeMap<&'static str, usize> {
    let sp = rel_target(bytes, &workbook_part(bytes), REL_STYLES).unwrap();
    style_entries(&parse(&part(bytes, &sp))).into_iter().map(|(k, v)| (k, v.len())).collect()
}

fn write(bytes: &[u8], at: (u32, u32), f: &Format) -> Vec<u8> {
    let mut wb = Workbook::from_bytes(bytes.to_vec()).unwrap();
    wb.worksheet().unwrap().cell_mut(at.0, at.1).unwrap().set_number_with_format(1.0, f).unwrap();
    wb.to_bytes().unwrap()
}

#[test]
fn growth_stops_on_repeat() {
    for (name, input, reserved) in [
        ("61605", read("poi/61605.xlsx"), 0),
        ("52348", read("poi/52348.xlsx"), 0),
        ("ExcelPivotTableSample", read("poi/ExcelPivotTableSample.xlsx"), 0),
        ("minimal", read("minimal"), 1),
        ("47889", phase2_fixture("47889.xlsx"), 1),
    ] {
        let f = full_format();
        let c0 = counts(&input);
        let o1 = write(&input, (40, 0), &f);
        let c1 = counts(&o1);
        assert_eq!(c1["cellXfs"], c0["cellXfs"].max(1) + 1, "{name}: one new xf");
        for t in ["numFmts", "fonts", "borders"] {
            assert!(c1[t] <= c0[t].max(1) + 1, "{name}: {t}");
        }
        assert!(c1["fills"] <= c0["fills"].max(2) + 1, "{name}: fills");
        if reserved == 1 && name == "minimal" {
            assert_eq!(c0["fills"], 1, "fixture_minimal has one fill");
            assert_eq!(c1["fills"], 3, "gray125 first, then the new fill");
        }
        // same Format again, same session
        let mut wb = Workbook::from_bytes(o1.clone()).unwrap();
        {
            let mut ws = wb.worksheet().unwrap();
            ws.cell_mut(41, 0).unwrap().set_number_with_format(2.0, &f).unwrap();
            ws.cell_mut(42, 0).unwrap().set_number_with_format(3.0, &f).unwrap();
        }
        let o2 = wb.to_bytes().unwrap();
        assert_eq!(counts(&o2), c1, "{name}: same format again adds nothing (reopened session)");
        // a second format sharing the font adds no font
        let g = Format::new().set_bold().set_font_color(Color::rgb(0xFF0000)).set_num_format("0.0");
        let o3 = write(&o2, (43, 0), &g);
        let c3 = counts(&o3);
        assert_eq!(c3["fonts"], c1["fonts"], "{name}: shared font reused");
        assert_eq!(c3["cellXfs"], c1["cellXfs"] + 1, "{name}");
        if name == "52348" {
            // no cellStyleXfs: xfId omitted
            let sp = rel_target(&o1, "xl/workbook.xml", REL_STYLES).unwrap();
            let doc = parse(&part(&o1, &sp));
            let last = table(doc.root(), "cellXfs").unwrap().elements().last().unwrap();
            assert_eq!(last.attr(QName::new("", "xfId")), None, "52348: no dangling xfId");
        }
    }
}

#[test]
fn style_alternate_content() {
    let input = read("poi/style-alternate-content.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let e = ws.cell_mut(30, 0).unwrap().set_number_with_format(1.0, &Format::new().set_bold()).unwrap_err();
        assert!(matches!(e, Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{e:?}");
    }
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(30, 0).unwrap().set_style_index(28).unwrap();
    assert!(matches!(ws.cell_mut(30, 0).unwrap().set_style_index(29), Err(Error::StyleIndexOutOfRange { index: 29, count: 29 })));
    ws.cell_mut(31, 0).unwrap().set_string("new in hancell").unwrap();
    let out = wb.to_bytes().unwrap();
    let cr = calamine_cells(&out);
    assert!(cr.values().any(|s| s.get(&(31, 0)) == Some(&Data::String("new in hancell".into()))));
}

#[test]
fn ac_that_is_not_index_stable_refuses_index_operations() {
    // a Choice with two xf: indices behind it depend on the reader (§2, n9)
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let xf = r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#;
    let s = styles_xml(&base).replace(
        &format!(r#"<cellXfs count="1">{xf}</cellXfs>"#),
        &format!(r#"<cellXfs count="2">{xf}<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x14">{xf}{xf}</mc:Choice><mc:Fallback>{xf}</mc:Fallback></mc:AlternateContent></cellXfs>"#),
    );
    assert!(s.contains("mc:Choice"));
    let input = p1::with_part(&base, "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_style_index(0));
    assert!(matches!(e, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))), "{e:?}");
    let e = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::new()));
    assert!(matches!(e, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

#[test]
fn custom_numfmt_with_extra_attributes_is_not_reused() {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let code = NEW_CODE.replace('"', "&quot;");
    let s = styles_xml(&base).replace(
        "<fonts",
        &format!(r#"<numFmts count="2"><numFmt xmlns:x="urn:example" x:tag="1" numFmtId="170" formatCode="{code}"/><numFmt numFmtId="171" formatCode="0.0000"/></numFmts><fonts"#),
    );
    let input = p1::with_part(&base, "xl/styles.xml", &s);
    let out = write(&input, (0, 0), &Format::new().set_num_format(NEW_CODE));
    let e = style_entries(&parse(&part(&out, "xl/styles.xml")));
    assert_eq!(e["numFmts"].len(), 3, "170 has another attribute: not the exact form, so a new id");
    assert!(e["numFmts"][2].contains(r#"numFmtId="172""#), "{:?}", e["numFmts"]);
    let out = write(&input, (0, 0), &Format::new().set_num_format("0.0000"));
    assert_eq!(style_entries(&parse(&part(&out, "xl/styles.xml")))["numFmts"].len(), 2, "171 is reused");
}

#[test]
fn sst_with_alternate_content_refuses_shared_writes() {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let sst = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" count="0" uniqueCount="2"><si><t>a</t></si><mc:AlternateContent><mc:Choice Requires="x14"><si><t>b</t></si></mc:Choice><mc:Fallback><si><t>b</t></si></mc:Fallback></mc:AlternateContent></sst>"#;
    let input = p1::with_part(&base, "xl/sharedStrings.xml", sst);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    for text in ["new", "a"] {
        let e = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_string(text));
        assert!(matches!(e, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))), "{text}: {e:?}");
    }
    // the inline form is unaffected
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_string_inline("new").unwrap();
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &["xl/worksheets/sheet1.xml"]);
}

#[test]
fn stripped_shared_string_formula_results_leave_the_count() {
    // a formula cell typed t="s" (as some producers write it): Phase1 strips
    // its cached value and type, so it no longer counts (§4.5)
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let sst = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1"><si><t>x</t></si></sst>"#;
    let b = p1::with_part(&base, "xl/sharedStrings.xml", sst);
    let input = p1::with_part(&b, "xl/worksheets/sheet1.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1" t="s"><f>A1</f><v>0</v></c></row></sheetData></worksheet>"#);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_string("y").unwrap();
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    assert!(!xml.contains(r#"<c r="B1" t="s">"#), "B1's cache is stripped: {xml}");
    let root = parse(&part(&out, "xl/sharedStrings.xml"));
    assert_eq!(root.root().attr(QName::new("", "count")), Some("1"), "A1 only");
    assert_eq!(count_s_refs_by_content_type(&out), 1);
}

// ------------------------------------------------------------ shared strings

#[test]
fn shared_strings_reuse_and_counts() {
    // 64420: the macro sheet holds one of the three t="s" cells
    let input = phase2_fixture("64420.xlsm");
    let sst = |b: &[u8]| parse(&part(b, "xl/sharedStrings.xml"));
    let before = sst(&input);
    let n0 = sst_entries(&before).len();
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(wb.shared_strings_available().unwrap());
    {
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(20, 0).unwrap().set_string("p2 one").unwrap();
        ws.cell_mut(21, 0).unwrap().set_string("p2 one").unwrap();
        ws.cell_mut(22, 0).unwrap().set_string(" p2 padded ").unwrap();
        // overwrite a shared-string cell with a number: count drops
        ws.cell_mut(21, 0).unwrap().set_number(5.0).unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let after = sst(&out);
    let e = sst_entries(&after);
    assert_eq!(e.len(), n0 + 2, "one si per distinct string");
    assert_eq!(e[n0], "<si><t>p2 one</t></si>");
    assert_eq!(e[n0 + 1], r#"<si><t xml:space="preserve"> p2 padded </t></si>"#);
    assert_eq!(after.root().attr(QName::new("", "count")).unwrap().parse::<u64>().unwrap(), count_s_refs_by_content_type(&out));
    assert_eq!(after.root().attr(QName::new("", "uniqueCount")).unwrap(), (n0 + 2).to_string());
    // reopened: an existing exact-form si is reused
    let mut wb = Workbook::from_bytes(out.clone()).unwrap();
    wb.worksheet().unwrap().cell_mut(23, 0).unwrap().set_string("p2 one").unwrap();
    let out2 = wb.to_bytes().unwrap();
    assert_eq!(sst_entries(&sst(&out2)).len(), n0 + 2);
    let xml = sheet_xml(&out2);
    assert!(xml.contains(&format!(r#"<c r="A24" t="s"><v>{n0}</v></c>"#)), "{xml}");
}

#[test]
fn rich_or_phonetic_si_is_never_reused() {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let sst = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="3"><si><r><t>rich</t></r></si><si><t>phon</t><phoneticPr fontId="0"/></si><si><t>plain</t></si></sst>"#;
    let input = p1::with_part(&base, "xl/sharedStrings.xml", sst);
    let mut wb = Workbook::from_bytes(input).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(0, 0).unwrap().set_string("rich").unwrap();
        ws.cell_mut(0, 1).unwrap().set_string("phon").unwrap();
        ws.cell_mut(0, 2).unwrap().set_string("plain").unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<c r="A1" t="s"><v>3</v></c>"#) && xml.contains(r#"<c r="B1" t="s"><v>4</v></c>"#), "{xml}");
    assert!(xml.contains(r#"<c r="C1" t="s"><v>2</v></c>"#), "plain exact-form reused: {xml}");
    let s = text(&out, "xl/sharedStrings.xml");
    assert!(s.contains(r#"count="3" uniqueCount="5""#), "{s}");
}

#[test]
fn template_has_an_empty_sst_and_inline_fallback() {
    let mut wb = Workbook::new().unwrap();
    assert!(wb.shared_strings_available().unwrap());
    wb.add_worksheet().unwrap().write_string(0, 0, "a").unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(text(&out, "xl/sharedStrings.xml").contains(r#"count="1" uniqueCount="1"><si><t>a</t></si></sst>"#));
    // 47889 has no sst: inline
    let mut wb = Workbook::from_bytes(phase2_fixture("47889.xlsx")).unwrap();
    assert!(!wb.shared_strings_available().unwrap());
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_string("x").unwrap();
    assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(r#"t="inlineStr""#));
}

// ------------------------------------------------------------ styles: rules

fn styles_xml(bytes: &[u8]) -> String {
    text(bytes, "xl/styles.xml")
}

#[test]
fn reserved_fill_positions_are_not_reused() {
    // 64450: fills[1] is lightGray, byte-identical to a canonical lightGray fill
    let input = phase2_fixture("64450.xlsx");
    let f = Format { fill: Some(Fill::Pattern { pattern: "lightGray".into(), fg: None, bg: None }), ..Format::new() };
    let out = write(&input, (50, 0), &f);
    let doc = parse(&part(&out, "xl/styles.xml"));
    let fills = &style_entries(&doc)["fills"];
    assert_eq!(fills.len(), 3, "a new lightGray fill, not fills[1]");
    assert_eq!(fills[2], r#"<fill><patternFill patternType="lightGray"/></fill>"#);
    // NoFill reuses fills[0] (canonical none)
    let out = write(&input, (50, 0), &Format { fill: Some(Fill::NoFill), ..Format::new() });
    assert_eq!(style_entries(&parse(&part(&out, "xl/styles.xml")))["fills"].len(), 2);
}

#[test]
fn xf_projection_collisions_append() {
    // two xfs that differ from the canonical form only in xfId / apply flags
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let s = styles_xml(&base).replace(
        r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
        r#"<cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="2" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1" applyFont="1"/><xf numFmtId="2" fontId="0" fillId="0" borderId="0"/></cellXfs>"#,
    );
    let input = p1::with_part(&base, "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("0.00")).unwrap();
    let out = wb.to_bytes().unwrap();
    let xfs = &style_entries(&parse(&part(&out, "xl/styles.xml")))["cellXfs"];
    assert_eq!(xfs.len(), 4);
    assert_eq!(xfs[3], r#"<xf numFmtId="2" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#);
    assert!(sheet_xml(&out).contains(r#"<c r="A1" s="3">"#));
    // and exactly that form is reused
    let mut wb = Workbook::from_bytes(out).unwrap();
    wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_number_with_format(1.0, &Format::new().set_num_format_index(2)).unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(style_entries(&parse(&part(&out, "xl/styles.xml")))["cellXfs"].len(), 4);
}

#[test]
fn numfmt_ids_are_above_every_id_in_the_package() {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    // a pivot-like part using numFmtId 177 (with odd spacing and quotes)
    let mut pkg = ooxml_core::Package::from_bytes(base).unwrap();
    pkg.set_part("xl/worksheets/sheet1.xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><extLst><ext uri="u"><x numFmtId = '177'/></ext></extLst></worksheet>"#.to_vec()).unwrap();
    let input = pkg.to_bytes().unwrap();
    let out = write(&input, (0, 0), &Format::new().set_num_format(NEW_CODE));
    assert!(styles_xml(&out).contains(&format!(r#"<numFmts count="1"><numFmt numFmtId="178" formatCode="{}"/></numFmts>"#, NEW_CODE.replace('"', "&quot;"))), "{}", styles_xml(&out));
    // built-in code maps to its id; a redefined built-in id is refused
    let out = write(&input, (0, 0), &Format::new().set_num_format("0.00%"));
    assert!(styles_xml(&out).contains(r#"<xf numFmtId="10" "#));
    let s = styles_xml(&input).replace("<fonts", r#"<numFmts count="1"><numFmt numFmtId="14" formatCode="yyyy/m/d"/></numFmts><fonts"#);
    let red = p1::with_part(&input, "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(red.clone()).unwrap();
    let e = wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::date());
    assert!(matches!(e, Err(Error::BuiltinNumFormatRedefined { id: 14 })), "{e:?}");
    assert_l1_except(&red, &wb.to_bytes().unwrap(), &[]);
    // set_datetime falls back to yyyy-mm-dd there
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_datetime(ExcelDateTime::from_ymd(2026, 11, 16).unwrap()).unwrap();
    assert!(styles_xml(&wb.to_bytes().unwrap()).contains(r#"formatCode="yyyy-mm-dd""#));
}

#[test]
fn style_table_limits() {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let font = r#"<font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>"#;
    let fonts: String = (0..512).map(|i| font.replace("11", &format!("{}", 8 + i % 300))).collect();
    let s = styles_xml(&base).replace(&format!(r#"<fonts count="1">{font}</fonts>"#), &format!(r#"<fonts count="512">{fonts}</fonts>"#));
    let input = p1::with_part(&base, "xl/styles.xml", &s);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::new().set_font_name("Arial"));
    assert!(matches!(e, Err(Error::StyleTableFull { ref table, limit: 512 }) if table == "fonts"), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    // reuse still works
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("0.0")).unwrap();
    // a table already past its limit does not block writes that do not grow it
    let over = s.replace(r#"<fonts count="512">"#, r#"<fonts count="513">"#).replace("</fonts>", &format!("{font}</fonts>"));
    let input = p1::with_part(&base, "xl/styles.xml", &over);
    let mut wb = Workbook::from_bytes(input).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(0, 0).unwrap().set_style_index(0).unwrap();
    ws.cell_mut(0, 1).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("0.0")).unwrap();
    let e = ws.cell_mut(0, 2).unwrap().set_number_with_format(1.0, &Format::new().set_font_name("Arial"));
    assert!(matches!(e, Err(Error::StyleTableFull { .. })), "{e:?}");
}

#[test]
fn dangling_references_refuse_appends() {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let input = p1::with_part(&base, "xl/worksheets/sheet1.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="3"><v>1</v></c></row></sheetData></worksheet>"#);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_number_with_format(1.0, &Format::new().set_bold());
    assert!(matches!(e, Err(Error::DanglingReference { ref table, index: 3 }) if table == "cellXfs"), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    // applying an existing index is still allowed
    wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_style_index(0).unwrap();
}

#[test]
fn no_styles_part() {
    let input = p1::book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_format(&Format::new().set_bold());
    assert!(matches!(e, Err(Error::NoStylesPart)), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

// ------------------------------------------------------------ preserving setters and named styles

const STYLED: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="10"/><name val="Arial"/><family val="2"/></font><font><b/><sz val="10"/><name val="Arial"/><family val="2"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" applyNumberFormat="0"><alignment horizontal="center"/></xf></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="3" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1" applyNumberFormat="1"><alignment vertical="top"/></xf></cellXfs><cellStyles count="2"><cellStyle name="Normal" xfId="0" builtinId="0"/><cellStyle name="Good" xfId="1" builtinId="26"/></cellStyles></styleSheet>"#;

fn styled_book() -> Vec<u8> {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let b = p1::with_part(&base, "xl/styles.xml", STYLED);
    p1::with_part(&b, "xl/worksheets/sheet1.xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="1"><v>1234</v></c><c r="B1"><v>2</v></c></row></sheetData></worksheet>"#)
}

#[test]
fn num_format_only_keeps_the_rest_of_the_style() {
    let mut wb = Workbook::from_bytes(styled_book()).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_num_format("0.00%").unwrap();
    let out = wb.to_bytes().unwrap();
    let xfs = &style_entries(&parse(&part(&out, "xl/styles.xml")))["cellXfs"];
    assert_eq!(xfs[2], r#"<xf numFmtId="10" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1" applyNumberFormat="1"><alignment vertical="top"/></xf>"#);
    assert!(sheet_xml(&out).contains(r#"<c r="A1" s="2"><v>1234</v></c>"#), "value kept");
}

#[test]
fn font_fill_border_only_keep_the_rest_of_the_style() {
    use ooxml_xlsx::{Border, Font, Side};
    let mut wb = Workbook::from_bytes(styled_book()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let italic = Font { italic: true, ..Font::default() };
        ws.cell_mut(0, 0).unwrap().set_font(&italic).unwrap();
        ws.cell_mut(0, 0).unwrap().set_fill(&Fill::Pattern { pattern: "solid".into(), fg: Some(Color::rgb(0x00FF00)), bg: None }).unwrap();
        let side = Some(Side { style: "thin".into(), color: None });
        ws.cell_mut(0, 0).unwrap().set_border(&Border { bottom: side, ..Border::default() }).unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let e = style_entries(&parse(&part(&out, "xl/styles.xml")));
    // each step copies the previous xf and replaces one id + its apply flag
    assert_eq!(e["cellXfs"][2], r#"<xf numFmtId="3" fontId="2" fillId="0" borderId="0" xfId="0" applyFont="1" applyNumberFormat="1"><alignment vertical="top"/></xf>"#);
    assert_eq!(e["cellXfs"][3], r#"<xf numFmtId="3" fontId="2" fillId="2" borderId="0" xfId="0" applyFont="1" applyNumberFormat="1" applyFill="1"><alignment vertical="top"/></xf>"#);
    assert_eq!(e["cellXfs"][4], r#"<xf numFmtId="3" fontId="2" fillId="2" borderId="1" xfId="0" applyFont="1" applyNumberFormat="1" applyFill="1" applyBorder="1"><alignment vertical="top"/></xf>"#);
    assert!(sheet_xml(&out).contains(r#"<c r="A1" s="4"><v>1234</v></c>"#), "value kept");
}

#[test]
fn preserving_edits_refuse_what_they_cannot_carry() {
    // an xf with an extension attribute cannot be copied without losing it
    let styled = STYLED.replace(r#"<xf numFmtId="3" fontId="1""#, r#"<xf xmlns:x14ac="http://schemas.microsoft.com/office/spreadsheetml/2009/9/ac" x14ac:foo="1" numFmtId="3" fontId="1""#);
    let input = p1::with_part(&styled_book(), "xl/styles.xml", &styled);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_num_format("0.0"));
    assert!(matches!(e, Err(Error::NotExpressible(_))), "{e:?}");
    // an out-of-range s is not derived from the default style
    let input = patch_part(&styled_book(), "xl/worksheets/sheet1.xml", r#"<c r="A1" s="1">"#, r#"<c r="A1" s="7">"#);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let e = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_font(&ooxml_xlsx::Font::default()));
    assert!(matches!(e, Err(Error::DanglingReference { index: 7, .. })), "{e:?}");
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

#[test]
fn invalid_format_values_are_refused() {
    let input = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    for f in [
        Format::new().set_border("hairline"),
        Format::new().set_align("middle"),
        Format::new().set_valign("centre"),
        Format { fill: Some(Fill::Pattern { pattern: "Solid".into(), fg: None, bg: None }), ..Format::new() },
        Format::new().set_font_name(""),
    ] {
        let e = guarded(&mut wb, |wb| wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(1.0, &f));
        assert!(matches!(e, Err(Error::InvalidFormat(_))), "{f:?}: {e:?}");
    }
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    assert!(ooxml_xlsx::FontSize::new(0.5).is_none() && ooxml_xlsx::FontSize::new(f64::NAN).is_none());
    assert_eq!(Format::new().set_font_size(500.0), Format::new(), "a rejected size leaves the format unchanged");
}

#[test]
fn quote_prefix_bool_and_error() {
    let mut wb = Workbook::new().unwrap();
    {
        let mut ws = wb.add_worksheet().unwrap();
        ws.cell_mut(0, 0).unwrap().set_string_with_format("=not a formula", &Format::new().set_quote_prefix()).unwrap();
        ws.cell_mut(0, 1).unwrap().set_bool(true).unwrap();
        ws.cell_mut(0, 2).unwrap().set_error("#N/A").unwrap();
        assert!(matches!(ws.cell_mut(0, 3).unwrap().set_error("#WHAT"), Err(Error::InvalidValue { .. })));
    }
    let out = wb.to_bytes().unwrap();
    let xfs = &style_entries(&parse(&part(&out, "xl/styles.xml")))["cellXfs"];
    assert_eq!(xfs[1], r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" quotePrefix="1"/>"#);
    let cells = calamine_cells(&out);
    let sheet = cells.values().next().unwrap();
    assert_eq!(sheet.get(&(0, 1)), Some(&Data::Bool(true)));
    assert_eq!(sheet.get(&(0, 2)), Some(&Data::Error(calamine::CellErrorType::NA)));
}

#[test]
fn named_style_respects_apply_flags() {
    let mut wb = Workbook::from_bytes(styled_book()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(0, 0).unwrap().set_named_style("good").unwrap();
        assert!(matches!(ws.cell_mut(0, 0).unwrap().set_named_style("Bad"), Err(Error::NoSuchStyle(_))));
    }
    let out = wb.to_bytes().unwrap();
    let xfs = &style_entries(&parse(&part(&out, "xl/styles.xml")))["cellXfs"];
    // number format (applyNumberFormat="0") stays the cell's 3; the rest from the style
    assert_eq!(xfs[2], r#"<xf numFmtId="3" fontId="1" fillId="0" borderId="0" xfId="1" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1" applyProtection="1"><alignment horizontal="center"/></xf>"#);
}

#[test]
fn inherit_and_default_font() {
    let mut wb = Workbook::from_bytes(styled_book()).unwrap();
    let f = Format::with_default_font_of(&mut wb).unwrap().set_bold();
    assert_eq!(f.font.as_ref().unwrap().name, "Arial");
    {
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(1, 0).unwrap().set_number_with_format(1.0, &f).unwrap();
        ws.cell_mut(1, 1).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("0.0")).unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let e = style_entries(&parse(&part(&out, "xl/styles.xml")));
    assert_eq!(e["fonts"].len(), 2, "Arial 10 bold reuses fonts[1] byte-identically");
    assert_eq!(e["cellXfs"][2], r#"<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#);
    assert_eq!(e["cellXfs"][3], r#"<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#);
}

// ------------------------------------------------------------ dates

#[test]
fn dates_in_both_systems() {
    for date1904 in [false, true] {
        let base = Workbook::new().unwrap().to_bytes().unwrap();
        let input = if date1904 { patch_part(&base, "xl/workbook.xml", "<workbookPr ", r#"<workbookPr date1904="1" "#) } else { base };
        let mut wb = Workbook::from_bytes(input).unwrap();
        assert_eq!(wb.date1904().unwrap(), date1904);
        let d = ExcelDateTime::from_ymd_hms(2026, 11, 16, 12, 0, 0.0).unwrap();
        {
            let mut ws = wb.worksheet().unwrap();
            ws.cell_mut(0, 0).unwrap().set_datetime(d).unwrap();
            ws.cell_mut(0, 1).unwrap().set_datetime(ExcelDateTime::from_ymd(1900, 3, 1).unwrap()).map(|_| ()).unwrap_or_else(|e| assert!(date1904 && matches!(e, Error::DateOutOfRange)));
            assert!(ws.is_date(0, 0).unwrap());
            assert_eq!(ws.datetime(0, 0).unwrap(), Some(d));
            // an existing date format is kept
            ws.cell_mut(0, 2).unwrap().set_number_with_format(1.0, &Format::new().set_num_format("yyyy\\-mm\\-dd hh:mm")).unwrap();
            ws.cell_mut(0, 2).unwrap().set_datetime(d).unwrap();
        }
        let out = wb.to_bytes().unwrap();
        let xml = sheet_xml(&out);
        let serial = if date1904 { 46342.5 - 1462.0 } else { 46342.5 };
        assert!(xml.contains(&format!(r#"<c r="A1" s="1"><v>{serial}</v></c>"#)), "{xml}");
        assert!(xml.contains(r#"<c r="C1" s="2">"#), "{xml}");
        let mut xw: Xlsx<_> = Xlsx::new(Cursor::new(out.clone())).unwrap();
        let r = xw.worksheet_range("Sheet1").unwrap();
        match r.get_value((0, 0)) {
            Some(Data::DateTime(dt)) => assert_eq!(dt.as_f64(), serial),
            other => panic!("calamine reads {other:?}"),
        }
    }
    // serial 60 in the 1900 system
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(base).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(0, 0).unwrap().set_number_with_format(60.0, &Format::date()).unwrap();
    assert!(matches!(ws.datetime(0, 0), Err(Error::NonexistentDate)));
    assert!(ExcelDateTime::from_ymd(10000, 1, 1).and_then(|d| d.serial(false)).is_none());
}

// ------------------------------------------------------------ Phase1 interaction (§7)

fn formula_book(f: &str, names: &str) -> Vec<u8> {
    let base = Workbook::new().unwrap().to_bytes().unwrap();
    let b = p1::with_part(&base, "xl/worksheets/sheet1.xml", &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>3</v></c><c r="B1"><f>{f}</f><v>6</v></c></row></sheetData></worksheet>"#));
    if names.is_empty() { b } else { patch_part(&b, "xl/workbook.xml", "</sheets>", &format!("</sheets><definedNames>{names}</definedNames>")) }
}

fn b1_has_cache(bytes: &[u8]) -> bool {
    sheet_xml(bytes).contains("<v>6</v>")
}

#[test]
fn style_only_writes_and_format_reading_functions() {
    // plain formula: a style-only write invalidates nothing
    let input = formula_book("A1*2", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_format(&Format::new().set_bold()).unwrap();
    let out = wb.to_bytes().unwrap();
    assert!(b1_has_cache(&out));
    assert!(!text(&out, "xl/workbook.xml").contains("fullCalcOnLoad"));
    // CELL in a formula: treated as a value write
    for (f, names) in [
        (r#"CELL("format",A1)"#, ""),
        ("A1*2", r#"<definedName name="fmt">GET.CELL(7,Sheet1!A1)</definedName>"#),
        ("MYUDF(A1)", ""),
    ] {
        let input = formula_book(&f.replace('"', "&quot;"), names);
        let mut wb = Workbook::from_bytes(input).unwrap();
        wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_format(&Format::new().set_bold()).unwrap();
        let out = wb.to_bytes().unwrap();
        assert!(!b1_has_cache(&out), "{f} / {names}: dependent cache stripped");
        assert!(text(&out, "xl/workbook.xml").contains(r#"fullCalcOnLoad="1""#), "{f}");
        assert!(sheet_xml(&out).contains(r#"<c r="A1" s="1"><v>3</v></c>"#), "value kept");
    }
    // a value write with a format strips dependents as set_number does
    let mut wb = Workbook::from_bytes(formula_book("A1*2", "")).unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number_with_format(4.0, &Format::new().set_bold()).unwrap();
    assert!(!b1_has_cache(&wb.to_bytes().unwrap()));
}

#[cfg(feature = "fault-injection")]
#[test]
fn a_failed_registry_apply_poisons() {
    use ooxml_xlsx::ApplyStep;
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    wb.inject_apply_failure(ApplyStep::Registries);
    let e = wb.worksheet().unwrap().cell_mut(5, 0).unwrap().set_number_with_format(1.0, &full_format());
    assert!(matches!(e, Err(Error::Poisoned(_))), "{e:?}");
    assert!(matches!(wb.to_bytes(), Err(Error::Poisoned(_))));
}

// ------------------------------------------------------------ from scratch vs rust_xlsxwriter (RX)

#[test]
fn from_scratch_matches_rust_xlsxwriter() {
    use ooxml_xlsx::Underline;
    use rust_xlsxwriter as rx;
    let red = 0xFF0000;
    let yellow = 0xFFFF00;
    let ours: Vec<Format> = vec![
        Format::new(),
        Format::new().set_bold(),
        Format::new().set_italic().set_underline(Underline::Single),
        Format::new().set_font_size(14.0).set_font_name("Arial"),
        Format::new().set_font_color(Color::rgb(red)),
        Format::new().set_background_color(Color::rgb(yellow)),
        Format::new().set_border("thin"),
        Format::new().set_num_format("0.00"),
        Format::new().set_num_format("0.000"),
        Format::new().set_align("center").set_text_wrap(),
        Format::new().set_unlocked().set_hidden(),
        Format::new().set_strikethrough(),
        Format::new().set_bold(),
        full_format().set_num_format("0.000"),
    ];
    let theirs: Vec<rx::Format> = vec![
        rx::Format::new(),
        rx::Format::new().set_bold(),
        rx::Format::new().set_italic().set_underline(rx::FormatUnderline::Single),
        rx::Format::new().set_font_size(14).set_font_name("Arial"),
        rx::Format::new().set_font_color(rx::Color::RGB(red)),
        rx::Format::new().set_background_color(rx::Color::RGB(yellow)),
        rx::Format::new().set_border(rx::FormatBorder::Thin),
        // ADR-0006 §5.3 maps a locale-independent built-in code to its id;
        // rust_xlsxwriter 0.102 gives every code a custom id from 164
        // (`workbook.rs` `prepare_num_formats`). The intended difference is
        // expressed here by asking rust_xlsxwriter for the built-in id.
        rx::Format::new().set_num_format_index(2),
        rx::Format::new().set_num_format("0.000"),
        rx::Format::new().set_align(rx::FormatAlign::Center).set_text_wrap(),
        rx::Format::new().set_unlocked().set_hidden(),
        rx::Format::new().set_font_strikethrough(),
        rx::Format::new().set_bold(),
        rx::Format::new()
            .set_num_format("0.000")
            .set_bold()
            .set_font_color(rx::Color::RGB(red))
            .set_background_color(rx::Color::RGB(yellow))
            .set_border(rx::FormatBorder::Thin),
    ];
    let mut wb = Workbook::new().unwrap();
    {
        let mut ws = wb.add_worksheet().unwrap();
        for (i, f) in ours.iter().enumerate() {
            ws.cell_mut(i as u32, 0).unwrap().set_number_with_format(i as f64, f).unwrap();
            ws.cell_mut(i as u32, 1).unwrap().set_string_with_format(&format!("s{}", i % 3), f).unwrap();
        }
    }
    let a = wb.to_bytes().unwrap();
    let mut rwb = rx::Workbook::new();
    {
        let ws = rwb.add_worksheet();
        for (i, f) in theirs.iter().enumerate() {
            ws.write_number_with_format(i as u32, 0, i as f64, f).unwrap();
            ws.write_string_with_format(i as u32, 1, format!("s{}", i % 3), f).unwrap();
        }
    }
    let b = rwb.save_to_buffer().unwrap();
    // styles.xml tables, entry by entry
    let (ea, eb) = (style_entries(&parse(&part(&a, "xl/styles.xml"))), style_entries(&parse(&part(&b, "xl/styles.xml"))));
    for t in ["numFmts", "fonts", "fills", "borders", "cellXfs"] {
        assert_eq!(ea[t], eb[t], "{t}");
    }
    // the built-in mapping itself (§5.3): "0.00" is id 2, with no numFmt entry
    assert!(ea["cellXfs"].iter().any(|x| x.contains("numFmtId=\"2\"")), "{:?}", ea["cellXfs"]);
    assert!(!ea["numFmts"].iter().any(|x| x.contains("formatCode=\"0.00\"")));
    // shared strings
    assert_eq!(sst_entries(&parse(&part(&a, "xl/sharedStrings.xml"))), sst_entries(&parse(&part(&b, "xl/sharedStrings.xml"))));
    // calamine: values and styles
    assert_eq!(calamine_cells(&a), calamine_cells(&b));
    let style = |bytes: &[u8]| {
        let mut x: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec())).unwrap();
        let s = x.worksheet_style("Sheet1").unwrap();
        (0..ours.len() as u32).flat_map(|r| (0..2).map(move |c| (r, c))).map(|k| format!("{:?}", s.get(k))).collect::<Vec<_>>()
    };
    assert_eq!(style(&a), style(&b));
}
