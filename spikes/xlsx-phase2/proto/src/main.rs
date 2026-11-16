//! ADR-0006 registry prototype (throwaway; not for merge as code).
//!
//! For every package given on the command line (files or directories), in
//! **edit mode**:
//!
//! 1. append one plain string to `sharedStrings.xml` (if present);
//! 2. append one style: a custom `numFmt` (id above every existing id,
//!    `numFmts` inserted if absent), a bold red font, a solid yellow fill
//!    (after padding fills 0/1 if missing), a thin border, and a `cellXfs`
//!    entry referencing them with `xfId="0"`;
//! 3. write a new last row on the first visible worksheet: `A` = the new
//!    shared string (`t="s"`), `B` = 1234.5678 with the new `xf`;
//! 4. save, then check
//!    (a) every pre-existing table entry is byte-identical at the same index
//!        and every pre-existing cell keeps its `s`/`t`/`v`;
//!    (b) byte locality: the registry parts differ only in the open tags of
//!        grown tables (`count`) and in the appended entries (T2-append);
//!    (c) calamine reads the new values and number format, and every other
//!        cell of every sheet as before (CR);
//!    (d) `count`/`uniqueCount` agree with the entries and references;
//!    plus L1 (content) for every other part.
//!
//! Prints one line per package and a summary; observations that contradict
//! an ADR-0006 assumption are tagged `ADR:`.

mod specs;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, Xlsx};
use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{EnumValue, LensMut, Package, QName, RawNode, XmlDocument};
use ooxml_schema::sml::{self, CellValues, SheetStateValues};
use specs::*;

const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_SST: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const REL_WS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const CT_WS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const NEW_TEXT: &str = "ooxml-rs ADR-0006 proto string";
const NEW_NUMBER: f64 = 1234.5678;
const NEW_CODE: &str = "0.000\" proto\"";
const COUNT: QName = QName::new("", "count");
const UNIQUE: QName = QName::new("", "uniqueCount");
const STYLE_TABLES: &[(&str, &str)] = &[
    ("numFmts", "numFmt"),
    ("fonts", "font"),
    ("fills", "fill"),
    ("borders", "border"),
    ("cellStyleXfs", "xf"),
    ("cellXfs", "xf"),
    ("cellStyles", "cellStyle"),
    ("dxfs", "dxf"),
];

fn x(local: &'static str) -> QName {
    QName::new(NS_X, local)
}

#[derive(Default, Debug)]
struct Report {
    file: String,
    skipped: Option<String>,
    refused: Vec<String>,
    failures: Vec<String>,
    notes: Vec<String>,
    string_added: bool,
    style_added: bool,
}

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> String + '_ {
    move |err| format!("{what}: {err}")
}

/// Table entries: element children named `child`, and `mc:AlternateContent`
/// children (one entry each after MCE).
fn entries<'n>(table: &'n RawNode, child: QName) -> Vec<&'n RawNode> {
    table.elements().filter(|n| n.is(child) || n.is(MC_ALTERNATE_CONTENT)).collect()
}

fn has_ac(table: &RawNode) -> bool {
    table.elements().any(|n| n.is(MC_ALTERNATE_CONTENT))
}

fn src<'s>(doc: &'s XmlDocument, n: &RawNode) -> &'s str {
    let s = n.span().expect("parsed node");
    &doc.source()[s.start..s.end()]
}

fn table<'d>(root: &'d RawNode, name: &'static str) -> Option<&'d RawNode> {
    root.elements().find(|n| n.is(x(name)))
}

/// (ref, s, t, v) of every cell of a worksheet.
type CellFacts = BTreeMap<String, (Option<String>, Option<String>, Option<String>)>;

fn cell_facts(doc: &XmlDocument) -> (CellFacts, u32, bool) {
    let (a, b, c, _) = cell_facts_rows(doc);
    (a, b, c)
}

fn cell_facts_rows(doc: &XmlDocument) -> (CellFacts, u32, bool, std::collections::BTreeSet<u32>) {
    let mut rows = std::collections::BTreeSet::new();
    let mut out = CellFacts::new();
    let mut max_row = 0;
    let mut row_without_r = false;
    if let Some(sd) = doc.root().elements().find(|n| n.is(x("sheetData"))) {
        for row in sd.elements().filter(|n| n.is(x("row"))) {
            match row.attr(QName::new("", "r")).and_then(|r| r.parse::<u32>().ok()) {
                Some(r) => {
                    max_row = max_row.max(r);
                    rows.insert(r);
                }
                None => row_without_r = true,
            }
            for c in row.elements().filter(|n| n.is(x("c"))) {
                let Some(r) = c.attr(QName::new("", "r")) else {
                    row_without_r = true;
                    continue;
                };
                let v = c.elements().find(|n| n.is(x("v"))).map(|v| v.text().into_owned());
                out.insert(
                    r.to_owned(),
                    (c.attr(QName::new("", "s")).map(str::to_owned), c.attr(QName::new("", "t")).map(str::to_owned), v),
                );
            }
        }
    }
    (out, max_row, row_without_r, rows)
}

fn count_s_refs(pkg: &Package, sheets: &[String]) -> Res<u32> {
    let mut n = 0;
    for s in sheets {
        let doc = XmlDocument::parse(&pkg.read_part(s).map_err(e(s))?).map_err(e(s))?;
        n += cell_facts(&doc).0.values().filter(|(_, t, _)| t.as_deref() == Some("s")).count() as u32;
    }
    Ok(n)
}

/// What the style edit produced.
struct StyleAdded {
    xf: u32,
    numfmt_id: u32,
    inserted_numfmts: bool,
    padded_fills: usize,
}

fn set_count<E: ooxml_core::ElementSpec>(l: &mut LensMut<'_, E>, n: usize, force: bool) -> Res<()> {
    if force || l.node().attr(COUNT).is_some() {
        l.set_attr(COUNT, &n.to_string()).map_err(e("count"))?;
    }
    Ok(())
}

fn add_style(doc: &mut XmlDocument, r: &mut Report, pkg_max_numfmt: Option<u32>) -> Res<Option<StyleAdded>> {
    let root = doc.root();
    if !root.is(x("styleSheet")) {
        r.refused.push(format!("styles root is {:?}", root.name()));
        return Ok(None);
    }
    for (t, _) in &STYLE_TABLES[..6] {
        if let Some(tb) = table(root, t)
            && has_ac(tb)
            && *t != "cellStyleXfs"
        {
            r.refused.push(format!("mc:AlternateContent inside {t} (ADR §2: table cannot grow)"));
            return Ok(None);
        }
    }
    if root.elements().any(|n| n.is(MC_ALTERNATE_CONTENT)) {
        r.notes.push("ADR: styleSheet has an mc:AlternateContent child".into());
    }
    // ids: numFmts and every numFmt anywhere (dxfs)
    let ids_numfmts: Vec<u32> = table(root, "numFmts")
        .map(|t| t.elements().filter_map(|n| n.attr(QName::new("", "numFmtId"))?.parse().ok()).collect())
        .unwrap_or_default();
    let mut ids_all = Vec::new();
    fn walk(n: &RawNode, out: &mut Vec<u32>) {
        if n.is(x("numFmt"))
            && let Some(i) = n.attr(QName::new("", "numFmtId")).and_then(|v| v.parse().ok())
        {
            out.push(i);
        }
        for c in n.elements() {
            walk(c, out);
        }
    }
    walk(root, &mut ids_all);
    let max_numfmts = ids_numfmts.iter().copied().max();
    let max_all = ids_all.iter().copied().max();
    if max_all > max_numfmts && max_all.is_some_and(|m| m >= 164) {
        r.notes.push(format!(
            "ADR: a dxf numFmt id ({max_all:?}) exceeds every numFmts id ({max_numfmts:?}); id allocation must scan dxfs too"
        ));
    }
    if pkg_max_numfmt > max_all && pkg_max_numfmt.is_some_and(|m| m >= 164) {
        r.notes.push(format!(
            "ADR M3: numFmtId {pkg_max_numfmt:?} is used outside styles.xml (pivot/extLst), above every styles.xml id ({max_all:?})"
        ));
    }
    let numfmt_id = max_all.max(pkg_max_numfmt).unwrap_or(0).max(163) + 1;
    if ids_all.iter().any(|&i| i < 164) {
        r.notes.push("custom numFmt id < 164 present".into());
    }
    let xfs_cellstyle = table(root, "cellStyleXfs").map_or(0, |t| entries(t, x("xf")).len());
    if xfs_cellstyle == 0 {
        r.notes.push("ADR: no cellStyleXfs entry, so a new xf must omit xfId (xfId=\"0\" would dangle)".into());
    }
    let n_fonts = table(root, "fonts").map_or(0, |t| entries(t, x("font")).len());
    let n_fills = table(root, "fills").map_or(0, |t| entries(t, x("fill")).len());
    let n_borders = table(root, "borders").map_or(0, |t| entries(t, x("border")).len());
    let n_xfs = table(root, "cellXfs").map_or(0, |t| entries(t, x("xf")).len());
    let had_numfmts = table(root, "numFmts").is_some();
    let created: Vec<&str> = ["fonts", "fills", "borders", "cellXfs"].into_iter().filter(|t| table(root, t).is_none()).collect();
    if !created.is_empty() {
        r.notes.push(format!("ADR: styles.xml lacks {created:?}; the registry has to insert them"));
    }
    let padded = 2usize.saturating_sub(n_fills);
    let pad_fonts = usize::from(n_fonts == 0);
    let pad_borders = usize::from(n_borders == 0);
    let pad_xfs = usize::from(n_xfs == 0);

    let mut ss = doc.lens_mut::<StyleSheet>().map_err(e("styleSheet"))?;
    {
        let mut t = ss.get_or_insert::<NumFmts>().map_err(e("insert numFmts"))?;
        let mut n = t.insert::<NumFmt>().map_err(e("numFmt"))?;
        n.set_attr(QName::new("", "numFmtId"), &numfmt_id.to_string()).map_err(e("numFmtId"))?;
        n.set_attr(QName::new("", "formatCode"), NEW_CODE).map_err(e("formatCode"))?;
        let len = ids_numfmts.len() + 1;
        set_count(&mut t, len, !had_numfmts)?;
    }
    {
        let mut t = ss.get_or_insert::<Fonts>().map_err(e("fonts"))?;
        if n_fonts == 0 {
            // index 0 is the workbook's default font: never give it to a new format
            let mut f = t.insert::<Font>().map_err(e("font"))?;
            f.insert::<FontSize>().map_err(e("sz"))?.set_attr(QName::new("", "val"), "11").map_err(e("sz"))?;
            f.insert::<FontName>().map_err(e("name"))?.set_attr(QName::new("", "val"), "Calibri").map_err(e("name"))?;
        }
        {
            let mut f = t.insert::<Font>().map_err(e("font"))?;
            f.insert::<Bold>().map_err(e("b"))?;
            f.insert::<FontSize>().map_err(e("sz"))?.set_attr(QName::new("", "val"), "11").map_err(e("sz"))?;
            f.insert::<Color>().map_err(e("color"))?.set_attr(QName::new("", "rgb"), "FFFF0000").map_err(e("rgb"))?;
            f.insert::<FontName>().map_err(e("name"))?.set_attr(QName::new("", "val"), "Calibri").map_err(e("name"))?;
        }
        set_count(&mut t, pad_fonts + n_fonts + 1, n_fonts == 0)?;
    }
    {
        let mut t = ss.get_or_insert::<Fills>().map_err(e("fills"))?;
        for k in n_fills..2 {
            let mut f = t.insert::<Fill>().map_err(e("fill"))?;
            let ty = if k == 0 { "none" } else { "gray125" };
            f.insert::<PatternFill>().map_err(e("pf"))?.set_attr(QName::new("", "patternType"), ty).map_err(e("pt"))?;
        }
        {
            let mut f = t.insert::<Fill>().map_err(e("fill"))?;
            let mut p = f.insert::<PatternFill>().map_err(e("pf"))?;
            p.set_attr(QName::new("", "patternType"), "solid").map_err(e("pt"))?;
            p.insert::<FgColor>().map_err(e("fg"))?.set_attr(QName::new("", "rgb"), "FFFFFF00").map_err(e("rgb"))?;
        }
        set_count(&mut t, n_fills + padded + 1, n_fills == 0)?;
    }
    {
        let mut t = ss.get_or_insert::<Borders>().map_err(e("borders"))?;
        if n_borders == 0 {
            let mut b = t.insert::<Border>().map_err(e("border"))?;
            for side in 0..5 {
                match side {
                    0 => { b.insert::<Left>().map_err(e("l"))?; }
                    1 => { b.insert::<Right>().map_err(e("r"))?; }
                    2 => { b.insert::<Top>().map_err(e("t"))?; }
                    3 => { b.insert::<Bottom>().map_err(e("b"))?; }
                    _ => { b.insert::<Diagonal>().map_err(e("d"))?; }
                }
            }
        }
        {
            let mut b = t.insert::<Border>().map_err(e("border"))?;
            b.insert::<Left>().map_err(e("left"))?.set_attr(QName::new("", "style"), "thin").map_err(e("s"))?;
            b.insert::<Right>().map_err(e("right"))?.set_attr(QName::new("", "style"), "thin").map_err(e("s"))?;
            b.insert::<Top>().map_err(e("top"))?.set_attr(QName::new("", "style"), "thin").map_err(e("s"))?;
            b.insert::<Bottom>().map_err(e("bottom"))?.set_attr(QName::new("", "style"), "thin").map_err(e("s"))?;
            b.insert::<Diagonal>().map_err(e("diagonal"))?;
        }
        set_count(&mut t, pad_borders + n_borders + 1, n_borders == 0)?;
    }
    {
        let mut t = ss.get_or_insert::<CellXfs>().map_err(e("cellXfs"))?;
        if n_xfs == 0 {
            // xf 0 is the style of every cell without `s`
            let mut xf = t.insert::<Xf>().map_err(e("xf"))?;
            for a in ["numFmtId", "fontId", "fillId", "borderId"] {
                xf.set_attr(QName::new("", a), "0").map_err(e(a))?;
            }
        }
        {
            let mut xf = t.insert::<Xf>().map_err(e("xf"))?;
            let mut attrs = vec![
                ("numFmtId", numfmt_id.to_string()),
                ("fontId", (pad_fonts + n_fonts).to_string()),
                ("fillId", (n_fills + padded).to_string()),
                ("borderId", (pad_borders + n_borders).to_string()),
            ];
            if xfs_cellstyle > 0 {
                attrs.push(("xfId", "0".into()));
            }
            for (a, v) in attrs.into_iter().chain([
                ("applyNumberFormat", "1".into()),
                ("applyFont", "1".into()),
                ("applyFill", "1".into()),
                ("applyBorder", "1".into()),
            ]) {
                xf.set_attr(QName::new("", a), &v).map_err(e(a))?;
            }
        }
        set_count(&mut t, pad_xfs + n_xfs + 1, n_xfs == 0)?;
    }
    for (what, n) in [("font", pad_fonts), ("border", pad_borders), ("xf", pad_xfs)] {
        if n > 0 {
            r.notes.push(format!("ADR: empty {what} table: a default entry had to be appended first so the new one does not become index 0 (the default)"));
        }
    }
    Ok(Some(StyleAdded { xf: (pad_xfs + n_xfs) as u32, numfmt_id, inserted_numfmts: !had_numfmts, padded_fills: padded }))
}

fn add_string(doc: &mut XmlDocument, r: &mut Report, refs_after: u32) -> Res<Option<u32>> {
    let root = doc.root();
    if !root.is(x("sst")) {
        r.refused.push("sst root".into());
        return Ok(None);
    }
    if has_ac(root) {
        r.refused.push("mc:AlternateContent inside sst".into());
        return Ok(None);
    }
    let n = entries(root, x("si")).len();
    let had_count = root.attr(COUNT).is_some();
    let had_unique = root.attr(UNIQUE).is_some();
    let mut sst = doc.lens_mut::<sml::SharedStringTable>().map_err(e("sst"))?;
    {
        let mut si = sst.insert::<sml::SharedStringItem>().map_err(e("si"))?;
        si.insert::<sml::Text>().map_err(e("t"))?.set_text(NEW_TEXT).map_err(e("t"))?;
    }
    if had_unique {
        sst.set_attr(UNIQUE, &(n + 1).to_string()).map_err(e("uniqueCount"))?;
    }
    if had_count {
        sst.set_attr(COUNT, &refs_after.to_string()).map_err(e("count"))?;
    }
    Ok(Some(n as u32))
}

/// T2-append: the diff between `a` and `b` is confined to, per grown table,
/// its open tag and the region of appended entries (or the whole table if it
/// was self-closing or newly inserted).
fn check_locality(a: &XmlDocument, b: &XmlDocument, tables: &[(&'static str, &'static str, usize)], root_is_table: Option<(&'static str, usize)>) -> Res<()> {
    let (ar, br) = (a.root(), b.root());
    let mut regions: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    if ar.span().unwrap().is_self_closing() {
        // a self-closing root is re-rendered whole (ADR-0002 §4.2)
        let (sa, sb) = (ar.span().unwrap(), br.span().unwrap());
        return if a.source()[..sa.start] == b.source()[..sb.start] && a.source()[sa.end()..] == b.source()[sb.end()..] {
            Ok(())
        } else {
            Err("bytes differ outside the re-rendered self-closing root".into())
        };
    }
    let one = |at: Option<&RawNode>, bt: &RawNode, child: &'static str, n_in: usize, regions: &mut Vec<_>| -> Res<()> {
        let bs = bt.span().unwrap();
        let Some(at) = at else {
            // inserted: in-point = start of the output's next sibling's input counterpart
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
        let be = entries(bt, x(child));
        let new = &be[n_in..];
        let (Some(first), Some(last)) = (new.first(), new.last()) else { return Ok(()) };
        let p = at.elements().find(|n| n.is(x("extLst"))).map_or(as_.close.unwrap().0, |n| n.span().unwrap().start);
        regions.push((p..p, first.span().unwrap().start..last.span().unwrap().end()));
        Ok(())
    };
    if let Some((child, n_in)) = root_is_table {
        one(Some(ar), br, child, n_in, &mut regions)?;
    }
    for &(name, child, n_in) in tables {
        let bt = table(br, name).ok_or(format!("{name} missing in output"))?;
        one(table(ar, name), bt, child, n_in, &mut regions)?;
    }
    regions.sort_by_key(|r| r.0.start);
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

/// Largest `numFmtId="N"` in any XML part other than `skip` (pivot tables,
/// pivot caches, worksheet extLst, …); a byte scan is enough for a maximum.
fn max_numfmt_id_in_package(pkg: &Package, skip: Option<&str>) -> Option<u32> {
    let mut best = None;
    for name in pkg.part_names() {
        if Some(name) == skip || !(name.ends_with(".xml")) {
            continue;
        }
        let Ok(b) = pkg.read_part(name) else { continue };
        let Ok(t) = std::str::from_utf8(&b) else { continue };
        for (i, _) in t.match_indices("numFmtId=\"") {
            let rest = &t[i + 10..];
            if let Some(end) = rest.find('"')
                && let Ok(v) = rest[..end].parse::<u32>()
            {
                best = best.max(Some(v));
            }
        }
    }
    best
}

/// M1 evidence: the default font (cellStyleXfs[0], else cellXfs[0]) of a styles part.
fn default_font_ids(doc: &XmlDocument) -> Option<(String, String, String)> {
    let root = doc.root();
    let xf = table(root, "cellStyleXfs")
        .and_then(|t| t.elements().find(|n| n.is(x("xf"))))
        .or_else(|| table(root, "cellXfs").and_then(|t| t.elements().find(|n| n.is(x("xf")))))?;
    let g = |a: &'static str| xf.attr(QName::new("", a)).unwrap_or("0").to_owned();
    Some((g("fontId"), g("fillId"), g("borderId")))
}

/// M2: canonical bytes of the entries this run appended (from the output).
fn appended_chain(doc: &XmlDocument, tables: &[(&'static str, &'static str, usize)]) -> Vec<(&'static str, Vec<String>)> {
    tables
        .iter()
        .map(|&(t, child, n_in)| {
            let ents = table(doc.root(), t).map(|n| entries(n, x(child))).unwrap_or_default();
            (t, ents[n_in.min(ents.len())..].iter().map(|n| src(doc, n).to_owned()).collect())
        })
        .collect()
}

fn calamine_all(bytes: &[u8]) -> Option<BTreeMap<String, BTreeMap<(u32, u32), Data>>> {
    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec())).ok()?;
    let mut out = BTreeMap::new();
    for name in wb.sheet_names().to_vec() {
        let Ok(range) = wb.worksheet_range(&name) else { continue };
        let mut m = BTreeMap::new();
        if let Some((r0, c0)) = range.start() {
            for (r, c, d) in range.used_cells() {
                m.insert((r0 + r as u32, c0 + c as u32), d.clone());
            }
        }
        out.insert(name, m);
    }
    Some(out)
}

fn run(path: &Path) -> Report {
    let mut r = Report { file: path.display().to_string(), ..Default::default() };
    if let Err(err) = run_inner(path, &mut r) {
        // unreadable input parts (DTD, bad CRC) are what Phase0 already refuses at open
        if err.contains("DTD") || err.contains("checksum") {
            r.skipped = Some(format!("input part unreadable: {err}"));
        } else {
            r.failures.push(err);
        }
    }
    r
}

fn run_inner(path: &Path, r: &mut Report) -> Res<()> {
    let input = std::fs::read(path).map_err(e("read"))?;
    let mut pkg = match Package::from_bytes(input.clone()) {
        Ok(p) => p,
        Err(err) => {
            r.skipped = Some(format!("open: {err}"));
            return Ok(());
        }
    };
    let Some(wb) = pkg.main_document_part() else {
        r.skipped = Some("no workbook".into());
        return Ok(());
    };
    let rels = pkg.relationships(&wb).map_err(e("rels"))?;
    let part_of = |ty: &str| rels.by_type(ty).next().and_then(|rel| rels.resolve(rel)).filter(|p| pkg.contains(p));
    let styles_part = part_of(REL_STYLES);
    let sst_part = part_of(REL_SST);
    let all_sheets: Vec<String> =
        rels.iter().filter(|rel| rel.rel_type == REL_WS).filter_map(|rel| rels.resolve(rel)).filter(|p| pkg.contains(p)).collect();
    let mut candidates = Vec::new();
    {
        let doc = match pkg.xml_part(&wb) {
            Ok(d) => d,
            Err(err) => {
                r.skipped = Some(format!("workbook.xml: {err}"));
                return Ok(());
            }
        };
        let Ok(l) = doc.lens::<sml::Workbook>() else {
            r.skipped = Some("not a Transitional workbook".into());
            return Ok(());
        };
        if let Some(sheets) = sml::WorkbookView(l).sheets() {
            for s in sheets.sheet_iter() {
                if !matches!(s.state(), EnumValue::Known(SheetStateValues::Visible)) {
                    continue;
                }
                let (Some(name), Some(id)) = (s.name(), s.id()) else { continue };
                candidates.push((name.to_owned(), id.to_owned()));
            }
        }
    }
    let target = candidates.into_iter().find_map(|(name, id)| {
        let rel = rels.by_id(&id).filter(|rel| rel.rel_type == REL_WS)?;
        let p = rels.resolve(rel)?;
        (pkg.content_type(&p) == Some(CT_WS)).then_some((name, p))
    });
    let Some((sheet_name, sheet_part)) = target else {
        r.skipped = Some("no visible worksheet".into());
        return Ok(());
    };
    let sheet_in = XmlDocument::parse(&pkg.read_part(&sheet_part).map_err(e("sheet"))?).map_err(e("sheet parse"))?;
    let (cells_in, max_row, row_without_r, used_rows) = cell_facts_rows(&sheet_in);
    if row_without_r {
        r.refused.push("row/cell without r (Phase0 MissingReference)".into());
        return Ok(());
    }
    let new_row = if max_row < 1_048_576 {
        max_row + 1
    } else {
        // the last row is used: take the first unused row number
        match (1..=1_048_576).find(|r| !used_rows.contains(r)) {
            Some(r) => r,
            None => {
                r.refused.push("every row used".into());
                return Ok(());
            }
        }
    };
    let refs_in = count_s_refs(&pkg, &all_sheets)?;
    let styles_in = match &styles_part {
        Some(p) => Some(XmlDocument::parse(&pkg.read_part(p).map_err(e("styles"))?).map_err(e("styles parse"))?),
        None => None,
    };
    let sst_in = match &sst_part {
        Some(p) => Some(XmlDocument::parse(&pkg.read_part(p).map_err(e("sst"))?).map_err(e("sst parse"))?),
        None => None,
    };

    // ---- edit
    let style = match &styles_part {
        Some(p) => {
            let m = max_numfmt_id_in_package(&pkg, Some(p.as_str()));
            add_style(pkg.xml_part_mut(p).map_err(e("styles"))?, r, m)?
        }
        None => {
            r.notes.push("no styles.xml (ADR §5.9: Err(NoStylesPart))".into());
            None
        }
    };
    let si = match &sst_part {
        Some(p) => add_string(pkg.xml_part_mut(p).map_err(e("sst"))?, r, refs_in + 1)?,
        None => {
            r.notes.push("no sharedStrings.xml (ADR §4.6: inline fallback)".into());
            None
        }
    };
    let mut inherit_xf = None;
    // the default style's font, read from the input XML (name, sz)
    let default_font: Option<(String, String)> = styles_in.as_ref().and_then(|d| {
        let (f, _, _) = default_font_ids(d)?;
        let font = table(d.root(), "fonts")?.elements().filter(|n| n.is(x("font"))).nth(f.parse().ok()?)?;
        let val = |l: &'static str| font.elements().find(|n| n.is(x(l)))?.attr(QName::new("", "val")).map(str::to_owned);
        Some((val("name")?, val("sz")?))
    });
    if let (Some(st), Some(p)) = (&style, &styles_part)
        && let Some((f, fi, b)) = styles_in.as_ref().and_then(default_font_ids)
    {
        let doc = pkg.xml_part_mut(p).map_err(e("styles"))?;
        let has_csx = table(doc.root(), "cellStyleXfs").is_some_and(|t| t.elements().next().is_some());
        let mut ss = doc.lens_mut::<StyleSheet>().map_err(e("ss"))?;
        let mut t = ss.get_or_insert::<CellXfs>().map_err(e("cellXfs"))?;
        let n = t.node().elements().filter(|n| n.is(x("xf")) || n.is(MC_ALTERNATE_CONTENT)).count();
        {
            let mut xf = t.insert::<Xf>().map_err(e("xf"))?;
            let mut attrs = vec![("numFmtId", st.numfmt_id.to_string()), ("fontId", f), ("fillId", fi), ("borderId", b)];
            if has_csx {
                attrs.push(("xfId", "0".into()));
            }
            attrs.push(("applyNumberFormat", "1".into()));
            for (a, v) in attrs {
                xf.set_attr(QName::new("", a), &v).map_err(e(a))?;
            }
        }
        if t.node().attr(COUNT).is_some() {
            t.set_attr(COUNT, &(n + 1).to_string()).map_err(e("count"))?;
        }
        inherit_xf = Some(n as u32);
    }
    r.style_added = style.is_some();
    r.string_added = si.is_some();
    {
        let doc = pkg.xml_part_mut(&sheet_part).map_err(e("sheet"))?;
        let mut ws = sml::WorksheetViewMut(doc.lens_mut::<sml::Worksheet>().map_err(e("ws"))?);
        let mut sd = match ws.get_or_insert_sheet_data() {
            Ok(sd) => sd,
            Err(err) => {
                r.refused.push(format!("sheetData: {err}"));
                return Ok(());
            }
        };
        let mut row = match sd.insert_row_by(|_, n| {
            n.attr(QName::new("", "r")).and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v > new_row)
        }) {
            Ok(row) => row,
            Err(err) => {
                r.refused.push(format!("row: {err}"));
                return Ok(());
            }
        };
        row.set_row_index(new_row).map_err(e("r"))?;
        {
            let mut a = row.insert_cell().map_err(e("A"))?;
            a.set_cell_reference(&format!("A{new_row}")).map_err(e("r"))?;
            match si {
                Some(i) => {
                    a.set_data_type(CellValues::SharedString).map_err(e("t"))?;
                    a.get_or_insert_cell_value().map_err(e("v"))?.set_text(&i.to_string()).map_err(e("v"))?;
                }
                None => {
                    a.set_data_type(CellValues::InlineString).map_err(e("t"))?;
                    a.get_or_insert_inline_string().map_err(e("is"))?.get_or_insert_text().map_err(e("t"))?.set_text(NEW_TEXT).map_err(e("t"))?;
                }
            }
        }
        {
            let mut b = row.insert_cell().map_err(e("B"))?;
            b.set_cell_reference(&format!("B{new_row}")).map_err(e("r"))?;
            if let Some(s) = &style {
                b.set_style_index(s.xf).map_err(e("s"))?;
            }
            b.get_or_insert_cell_value().map_err(e("v"))?.set_text(&NEW_NUMBER.to_string()).map_err(e("v"))?;
        }
        if let Some(xf_c) = inherit_xf {
            let mut c = row.insert_cell().map_err(e("C"))?;
            c.set_cell_reference(&format!("C{new_row}")).map_err(e("r"))?;
            c.set_style_index(xf_c).map_err(e("s"))?;
            c.get_or_insert_cell_value().map_err(e("v"))?.set_text(&NEW_NUMBER.to_string()).map_err(e("v"))?;
        }
    }
    let out = pkg.to_bytes().map_err(e("save"))?;
    if let Ok(dir) = std::env::var("PROTO_OUT") {
        let _ = std::fs::write(Path::new(&dir).join(path.file_name().unwrap()), &out);
    }
    let outp = Package::from_bytes(out.clone()).map_err(e("reopen"))?;

    // ---- L1 (content) for every other part
    for name in pkg.part_names() {
        if [Some(&sheet_part), styles_part.as_ref(), sst_part.as_ref()].contains(&Some(&name.to_owned())) {
            continue;
        }
        let orig = Package::from_bytes(input.clone()).map_err(e("reopen input"))?.read_part(name).map_err(e(name))?;
        if outp.read_part(name).map_err(e(name))? != orig {
            r.failures.push(format!("L1: {name} changed"));
        }
    }

    // ---- (a) index stability + (d) counts + (b) locality: styles
    if let (Some(a), Some(p), Some(st)) = (&styles_in, &styles_part, &style) {
        let b = XmlDocument::parse(&outp.read_part(p).map_err(e("styles out"))?).map_err(e("styles out parse"))?;
        let mut grown = Vec::new();
        for &(t, child) in STYLE_TABLES {
            let ea = table(a.root(), t).map(|n| entries(n, x(child))).unwrap_or_default();
            let eb = table(b.root(), t).map(|n| entries(n, x(child))).unwrap_or_default();
            for (i, (na, nb)) in ea.iter().zip(&eb).enumerate() {
                if src(a, na) != src(&b, nb) {
                    r.failures.push(format!("(a) styles {t}[{i}] changed"));
                }
            }
            if eb.len() < ea.len() {
                r.failures.push(format!("(a) styles {t} lost entries"));
            }
            if eb.len() > ea.len() {
                grown.push((t, child, ea.len()));
            }
            if let Some(tb) = table(b.root(), t) {
                let ok_in = table(a.root(), t).and_then(|n| n.attr(COUNT)).is_none_or(|c| c == ea.len().to_string());
                if !ok_in {
                    r.notes.push(format!("input {t}/@count wrong"));
                }
                if eb.len() > ea.len() && tb.attr(COUNT).is_some_and(|c| c != eb.len().to_string()) {
                    r.failures.push(format!("(d) {t}/@count {:?} != {}", tb.attr(COUNT), eb.len()));
                }
            }
        }
        // M2: a second session writing the same Format serialises byte-identical
        // font/fill/border entries (so exact-form reuse would hit); numFmt and xf
        // differ only in ids, so their reuse must be bottom-up and id-aware.
        {
            let first = appended_chain(&b, &grown);
            let mut p2 = Package::from_bytes(out.clone()).map_err(e("session 2"))?;
            let mut r2 = Report::default();
            let m = max_numfmt_id_in_package(&p2, Some(p.as_str()));
            let doc2 = p2.xml_part_mut(p).map_err(e("session 2 styles"))?;
            let before: Vec<_> = STYLE_TABLES
                .iter()
                .map(|&(t, c)| (t, c, table(doc2.root(), t).map_or(0, |n| entries(n, x(c)).len())))
                .collect();
            if add_style(doc2, &mut r2, m)?.is_some() {
                let b2 = XmlDocument::parse(doc2.serialize().as_bytes()).map_err(e("session 2 parse"))?;
                let second = appended_chain(&b2, &before);
                let norm = |t: &str, v: &str| -> String {
                    if t == "numFmts" || t == "cellXfs" {
                        let mut o = v.to_owned();
                        for a in ["numFmtId", "fontId", "fillId", "borderId"] {
                            if let Some(i) = o.find(&format!("{a}=\"")) {
                                let j = i + a.len() + 2;
                                let k = j + o[j..].find('"').unwrap();
                                o.replace_range(j..k, "#");
                            }
                        }
                        o
                    } else {
                        v.to_owned()
                    }
                };
                for ((t, v1), (_, v2)) in first.iter().zip(&second) {
                    let (v1, v2): (Vec<String>, Vec<String>) =
                        (v1.iter().map(|v| norm(t, v)).collect(), v2.iter().map(|v| norm(t, v)).collect());
                    if v2.last().is_some_and(|l| !v1.contains(l)) {
                        r.failures.push(format!("(M2) session-2 {t} entry differs: {:?} vs {:?}", v1.last(), v2.last()));
                    }
                }
                r.notes.push("M2: session-2 serialisation identical (ids aside)".into());
            }
        }
        if let Err(err) = check_locality(a, &b, &grown, None) {
            r.failures.push(format!("(b) styles: {err}"));
        }
        let n_xf_in = table(a.root(), "cellXfs").map_or(0, |t| entries(t, x("xf")).len());
        for s in &all_sheets {
            let doc = XmlDocument::parse(&pkg_part(&input, s)?).map_err(e(s))?;
            for (cr, (sv, _, _)) in cell_facts(&doc).0 {
                if let Some(sv) = sv
                    && sv.parse::<usize>().is_ok_and(|i| i >= n_xf_in)
                {
                    r.notes.push(format!("input anomaly: {s} {cr} s={sv} >= {n_xf_in} cellXfs (dangling; would now resolve to a NEW entry)"));
                }
            }
        }
        if st.inserted_numfmts {
            r.notes.push("numFmts inserted".into());
        }
        if st.padded_fills > 0 {
            r.notes.push(format!("padded {} mandatory fills", st.padded_fills));
        }
    }
    // ---- sst
    if let (Some(a), Some(p), Some(_)) = (&sst_in, &sst_part, si) {
        let b = XmlDocument::parse(&outp.read_part(p).map_err(e("sst out"))?).map_err(e("sst out parse"))?;
        let (ea, eb) = (entries(a.root(), x("si")), entries(b.root(), x("si")));
        for (i, (na, nb)) in ea.iter().zip(&eb).enumerate() {
            if src(a, na) != src(&b, nb) {
                r.failures.push(format!("(a) si[{i}] changed"));
            }
        }
        if eb.len() != ea.len() + 1 {
            r.failures.push("(a) si count".into());
        }
        if let Err(err) = check_locality(a, &b, &[], Some(("si", ea.len()))) {
            r.failures.push(format!("(b) sst: {err}"));
        }
        let refs_out = count_s_refs(&outp, &all_sheets)?;
        if a.root().attr(COUNT).is_some_and(|c| c != refs_in.to_string()) {
            r.notes.push(format!("input sst/@count {:?} != {refs_in} refs", a.root().attr(COUNT)));
        }
        if b.root().attr(COUNT).is_some_and(|c| c != refs_out.to_string()) {
            r.failures.push(format!("(d) sst/@count {:?} != {refs_out}", b.root().attr(COUNT)));
        }
        if b.root().attr(UNIQUE).is_some_and(|c| c != eb.len().to_string()) {
            r.failures.push(format!("(d) uniqueCount {:?} != {}", b.root().attr(UNIQUE), eb.len()));
        }
        for s in &all_sheets {
            let doc = XmlDocument::parse(&pkg_part(&input, s)?).map_err(e(s))?;
            for (cr, (_, t, v)) in cell_facts(&doc).0 {
                if t.as_deref() == Some("s") && v.as_deref().and_then(|v| v.trim().parse::<usize>().ok()).is_none_or(|i| i >= ea.len()) {
                    r.notes.push(format!("input anomaly: {s} {cr} shared-string index {v:?} invalid ({} si)", ea.len()));
                }
            }
        }
    }
    // pre-existing cells of the edited sheet keep s/t/v
    let sheet_out = XmlDocument::parse(&outp.read_part(&sheet_part).map_err(e("sheet out"))?).map_err(e("sheet out parse"))?;
    let (cells_out, _, _) = cell_facts(&sheet_out);
    for (cr, f) in &cells_in {
        if cells_out.get(cr) != Some(f) {
            r.failures.push(format!("(a) cell {cr} changed"));
        }
    }

    // ---- (c) calamine
    match (calamine_all(&input), calamine_all(&out)) {
        (Some(mut ci), Some(mut co)) => {
            let key_a = (new_row - 1, 0);
            let key_b = (new_row - 1, 1);
            if !ci.contains_key(&sheet_name) {
                r.notes.push("calamine cannot read the input sheet; (c) skipped".into());
                return Ok(());
            }
            match co.get_mut(&sheet_name) {
                Some(m) => {
                    match m.remove(&key_a) {
                        Some(Data::String(s)) if s == NEW_TEXT => {}
                        other => r.failures.push(format!("(c) A{new_row} reads {other:?}")),
                    }
                    match m.remove(&key_b) {
                        Some(Data::Float(f)) if f == NEW_NUMBER => {}
                        other => r.failures.push(format!("(c) B{new_row} reads {other:?}")),
                    }
                }
                None => r.failures.push("(c) calamine lost the sheet".into()),
            }
            if let Some(m) = ci.get_mut(&sheet_name) {
                m.remove(&key_a);
                m.remove(&key_b);
            }
            if inherit_xf.is_some()
                && let Some(m) = co.get_mut(&sheet_name)
                && !matches!(m.remove(&(new_row - 1, 2)), Some(Data::Float(f)) if f == NEW_NUMBER)
            {
                r.failures.push("(c) C reads wrong".into());
            }
            if ci != co {
                let diff: Vec<_> = ci.keys().filter(|k| ci.get(*k) != co.get(*k)).collect();
                r.failures.push(format!("(c) CR: other cells differ on {diff:?}"));
            }
            if let Some(st) = &style {
                let mut wbx: Xlsx<_> = Xlsx::new(Cursor::new(out.clone())).map_err(e("calamine"))?;
                match wbx.worksheet_style(&sheet_name) {
                    Ok(styles) => match styles.get(key_b) {
                        Some(s) => {
                            let code = s.number_format.as_ref().map(|n| n.format_code.clone());
                            if code.as_deref() != Some(NEW_CODE) {
                                r.failures.push(format!("(c) numFmt reads {code:?} (id {})", st.numfmt_id));
                            }
                            if !s.font.as_ref().is_some_and(|f| f.is_bold()) {
                                r.failures.push("(c) font not bold".into());
                            }
                            if inherit_xf.is_some() {
                                let font_of = |k: (u32, u32)| styles.get(k).and_then(|s| s.font.clone()).map(|f| (f.name, f.size));
                                let fa = default_font.clone().map(|(n, z)| (Some(n), z.parse::<f64>().ok()));
                                let fc = font_of((new_row - 1, 2));
                                if fa != fc {
                                    r.failures.push(format!("(M1) inherited font of C {fc:?} != default font {fa:?}"));
                                }
                                if fa.as_ref().is_some_and(|(n, sz)| n.as_deref() != Some("Calibri") || *sz != Some(11.0)) {
                                    r.notes.push(format!("M1: workbook default font is {fa:?}, not Calibri 11"));
                                }
                                let code = styles.get((new_row - 1, 2)).and_then(|s| s.number_format.clone()).map(|n| n.format_code);
                                if code.as_deref() != Some(NEW_CODE) {
                                    r.failures.push(format!("(M1) C numFmt reads {code:?}"));
                                }
                            }
                            let fg = s.fill.as_ref().and_then(|f| f.get_color()).map(|c| c.to_argb());
                            if fg.map(|c| c & 0xFFFFFF) != Some(0xFFFF00) {
                                r.failures.push(format!("(c) fill color {fg:X?}"));
                            }
                        }
                        None => r.failures.push("(c) no style at B".into()),
                    },
                    Err(err) => r.failures.push(format!("(c) worksheet_style: {err}")),
                }
            }
        }
        (None, _) => r.notes.push("calamine cannot open the input; (c) skipped".into()),
        (Some(_), None) => r.failures.push("(c) calamine cannot open the OUTPUT".into()),
    }
    Ok(())
}

fn pkg_part(bytes: &[u8], name: &str) -> Res<Vec<u8>> {
    Package::from_bytes(bytes.to_vec()).map_err(e("pkg"))?.read_part(name).map_err(e(name))
}

fn files(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            let mut v: Vec<PathBuf> = std::fs::read_dir(&p)
                .unwrap()
                .flatten()
                .flat_map(|e| {
                    let p = e.path();
                    if p.is_dir() { files(&[p.display().to_string()]) } else { vec![p] }
                })
                .filter(|p| p.extension().is_some_and(|e| e == "xlsx" || e == "xlsm"))
                .collect();
            v.sort();
            out.extend(v);
        } else {
            out.push(p);
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut tally = BTreeMap::<String, usize>::new();
    let mut failed = 0;
    for f in files(&args) {
        let r = std::panic::catch_unwind(|| run(&f)).unwrap_or_else(|_| Report {
            file: f.display().to_string(),
            failures: vec!["PANIC".into()],
            ..Default::default()
        });
        let status = if r.skipped.is_some() {
            "SKIP"
        } else if !r.failures.is_empty() {
            "FAIL"
        } else if !r.refused.is_empty() {
            "REFUSED"
        } else {
            "OK"
        };
        *tally.entry(status.into()).or_default() += 1;
        if r.string_added {
            *tally.entry("string appended".into()).or_default() += 1;
        }
        if r.style_added {
            *tally.entry("style appended".into()).or_default() += 1;
        }
        let mut notes = r.notes.clone();
        notes.sort();
        notes.dedup();
        for n in &notes {
            let key = n.split(':').take(2).collect::<Vec<_>>().join(":");
            let key = if key.starts_with("input anomaly") { "input anomaly".into() } else { key };
            *tally.entry(format!("note: {key}")).or_default() += 1;
        }
        for x in &r.refused {
            *tally.entry(format!("refused: {x}")).or_default() += 1;
        }
        if status == "FAIL" {
            failed += 1;
        }
        println!(
            "{status} {} {}{}{}",
            r.file,
            r.skipped.as_deref().unwrap_or(""),
            if r.failures.is_empty() { String::new() } else { format!(" failures={:?}", r.failures) },
            if notes.is_empty() && r.refused.is_empty() { String::new() } else { format!(" refused={:?} notes={:?}", r.refused, notes.iter().take(6).collect::<Vec<_>>()) },
        );
    }
    println!("--- summary");
    for (k, v) in tally {
        println!("{v:5}  {k}");
    }
    std::process::exit(i32::from(failed > 0));
}
