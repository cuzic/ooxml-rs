//! ADR-0014 §2's byte-exact allowed-diff oracle for Phase4 calls.
//!
//! An allowed change names a part, an element path (local names from the
//! part's root, `mergeCells/mergeCell`) and a kind. The oracle cuts every
//! allowed region out of the input and the output text, using Layer 2 spans
//! of both parsed trees, and requires the rest to be byte-identical; parts
//! that no row names must be identical zip entries.

use std::collections::BTreeMap;
use std::ops::Range;

use ooxml_core::{RawNode, XmlDocument};

use super::{entries, part};

/// Which part an allowed change is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The edited worksheet.
    Sheet,
    /// `xl/workbook.xml` (the main document part).
    Workbook,
    /// Phase1's composition (ADR-0003 §7) for cell clearing: `calcChain.xml`
    /// may disappear, with its `[Content_Types].xml` and workbook `.rels`
    /// entries.
    CalcChainRemoval,
    /// Other worksheets: only the listed cells may change (Phase1 strips).
    OtherSheetCells,
    /// The edited worksheet's `.rels` part (paths from `Relationships`);
    /// the part may appear when a row allows creating in it.
    SheetRels,
}

/// What may change at a path.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// Elements may be added; every input element (as text) is still there.
    Create,
    /// Elements may be removed; every output element was in the input.
    Remove,
    /// These attributes of the elements may change, appear or disappear.
    EditAttrs(&'static [&'static str]),
    /// A container: its own tags may change, appear or disappear (the
    /// children are judged by the other rows).
    Container,
    /// The elements' content (between their tags) may change.
    Content,
    /// The whole element may appear, disappear or change.
    Subtree,
    /// The `sheetData/row/c` elements at the cells given to the oracle may
    /// change (a composed Phase1 cell write or strip).
    Cells,
}

/// A value an [`Allowed::only`] selector compares an attribute with.
#[derive(Clone, Copy, Debug)]
pub enum Val {
    /// This exact text.
    Is(&'static str),
    /// The edited sheet's position in `<sheets>` (a `localSheetId`).
    ThisSheet,
}

/// A start tag's range and its normalized text.
type Norm = (Range<usize>, String);

/// One allowed change.
#[derive(Clone, Copy, Debug)]
pub struct Allowed {
    pub part: Part,
    pub path: &'static str,
    pub kind: Kind,
    /// Only the elements at `path` whose attributes have these values (all
    /// of them); the others must stay byte-identical. Empty: every element.
    pub only: &'static [(&'static str, Val)],
}

pub const fn sheet(path: &'static str, kind: Kind) -> Allowed {
    Allowed { part: Part::Sheet, path, kind, only: &[] }
}

pub const fn sheet_rels(path: &'static str, kind: Kind) -> Allowed {
    Allowed { part: Part::SheetRels, path, kind, only: &[] }
}

pub const fn workbook(path: &'static str, kind: Kind) -> Allowed {
    Allowed { part: Part::Workbook, path, kind, only: &[] }
}

impl Allowed {
    /// Narrows the row to the elements with these attribute values.
    pub const fn only(self, only: &'static [(&'static str, Val)]) -> Self {
        Allowed { only, ..self }
    }
}

fn at_path<'n>(root: &'n RawNode, path: &str) -> Vec<&'n RawNode> {
    let mut cur = vec![root];
    for seg in path.split('/') {
        cur = cur.iter().flat_map(|n| n.elements().filter(|e| e.name().local() == seg)).collect();
    }
    cur
}

/// The elements a row covers: at its path, and matching its selector.
fn selected<'n>(root: &'n RawNode, r: &Allowed, this_sheet: Option<&str>) -> Vec<&'n RawNode> {
    at_path(root, r.path)
        .into_iter()
        .filter(|n| {
            r.only.iter().all(|(a, v)| {
                let got = n.attr(ooxml_core::QName::new("", a));
                match v {
                    Val::Is(t) => got == Some(*t),
                    Val::ThisSheet => got.is_some() && got == this_sheet,
                }
            })
        })
        .collect()
}

/// The edited sheet's position in `<sheets>` (what `localSheetId` holds).
fn local_sheet_id(bytes: &[u8], sheet_part: &str) -> Option<String> {
    let rels = String::from_utf8(part(bytes, "xl/_rels/workbook.xml.rels")).ok()?;
    let rels = XmlDocument::parse(rels.as_bytes()).ok()?;
    let target = sheet_part.strip_prefix("xl/")?;
    let id = rels.root().elements().find(|r| r.attr(ooxml_core::QName::new("", "Target")).is_some_and(|t| t.trim_start_matches('/').trim_start_matches("xl/").eq_ignore_ascii_case(target)))?.attr(ooxml_core::QName::new("", "Id"))?.to_owned();
    let wb = String::from_utf8(part(bytes, "xl/workbook.xml")).ok()?;
    let wb = XmlDocument::parse(wb.as_bytes()).ok()?;
    let r_id = ooxml_core::QName::new("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id");
    let pos = at_path(wb.root(), "sheets/sheet").iter().position(|s| s.attr(r_id) == Some(id.as_str()))?;
    Some(pos.to_string())
}

fn whole(n: &RawNode) -> Range<usize> {
    let s = n.span().expect("parsed node");
    s.start..s.end()
}

fn open_tag(n: &RawNode) -> Range<usize> {
    let s = n.span().expect("parsed node");
    s.start..s.open_end
}

fn close_tag(n: &RawNode) -> Option<Range<usize>> {
    n.span().expect("parsed node").close.map(|(a, b)| a..b)
}

/// `src` with every range in `cut` removed, and the text of each range in
/// `normalize` replaced by `f(text)`.
fn residue(src: &str, cut: &mut Vec<Range<usize>>, normalize: &[(Range<usize>, String)]) -> String {
    // normalized tags first (they never overlap a cut range of the same
    // element; a cut range containing them wins)
    let mut edits: Vec<(Range<usize>, Option<&str>)> = normalize.iter().map(|(r, t)| (r.clone(), Some(t.as_str()))).collect();
    cut.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in cut.drain(..) {
        match merged.last_mut() {
            Some(m) if r.start < m.end => m.end = m.end.max(r.end),
            _ => merged.push(r),
        }
    }
    edits.retain(|(r, _)| !merged.iter().any(|m| m.start <= r.start && r.end <= m.end));
    edits.extend(merged.into_iter().map(|r| (r, None)));
    edits.sort_by_key(|(r, _)| r.start);
    let mut out = String::new();
    let mut pos = 0;
    for (r, t) in edits {
        if r.start < pos {
            continue;
        }
        out.push_str(&src[pos..r.start]);
        if let Some(t) = t {
            out.push_str(t);
        }
        pos = r.end;
    }
    out.push_str(&src[pos..]);
    out
}

fn strip_attrs(tag: &str, names: &[&str]) -> String {
    let mut t = tag.to_owned();
    for n in names {
        while let Some(i) = find_attr(&t, n) {
            let q = t[i..].find('"').map(|k| i + k).unwrap();
            let end = t[q + 1..].find('"').map(|k| q + 1 + k + 1).unwrap();
            let start = t[..i].trim_end_matches([' ', '\t', '\n', '\r']).len();
            t.replace_range(start..end, "");
        }
    }
    // a container emptied of attributes may be self-closing or not
    t
}

fn find_attr(tag: &str, name: &str) -> Option<usize> {
    let pat = format!("{name}=\"");
    let mut from = 0;
    while let Some(k) = tag[from..].find(&pat) {
        let i = from + k;
        if tag[..i].ends_with([' ', '\t', '\n', '\r']) {
            return Some(i);
        }
        from = i + 1;
    }
    None
}

/// The residue of one part's text under the allowed changes; also checks
/// the Create/Remove multiset rules.
fn judge(input: &str, output: &str, rows: &[Allowed], cells: &[String], this_sheet: Option<&str>) -> Result<(String, String), String> {
    let din = XmlDocument::parse(input.as_bytes()).map_err(|e| e.to_string())?;
    let dout = XmlDocument::parse(output.as_bytes()).map_err(|e| e.to_string())?;
    let mut cuts = (Vec::new(), Vec::new());
    let mut norms: (Vec<Norm>, Vec<Norm>) = (Vec::new(), Vec::new());
    for (doc, src, cut, norm) in [(&din, input, &mut cuts.0, &mut norms.0), (&dout, output, &mut cuts.1, &mut norms.1)] {
        for r in rows {
            for n in selected(doc.root(), r, this_sheet) {
                match r.kind {
                    Kind::EditAttrs(names) => {
                        let t = open_tag(n);
                        norm.push((t.clone(), strip_attrs(&src[t], names)));
                    }
                    Kind::Container => {
                        cut.push(open_tag(n));
                        if let Some(c) = close_tag(n) {
                            cut.push(c);
                        }
                    }
                    Kind::Content => {
                        if let Some(c) = close_tag(n) {
                            cut.push(open_tag(n).end..c.start);
                        } else {
                            // `<x/>` gaining content: compare the tag as
                            // open+close
                            cut.push(open_tag(n));
                        }
                    }
                    Kind::Create | Kind::Remove | Kind::Subtree => cut.push(whole(n)),
                    Kind::Cells => {
                        for row in n.elements().filter(|e| e.name().local() == "row") {
                            for c in row.elements().filter(|e| e.name().local() == "c") {
                                if c.attr(ooxml_core::QName::new("", "r")).is_some_and(|v| cells.iter().any(|x| x == v)) {
                                    cut.push(whole(c));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Create / Remove: compare the elements' texts as multisets. An
    // element's key is its text with every allowed attribute change inside
    // it normalized away, and with the descendants that deeper rows judge
    // (Create, Remove, Subtree) cut out: a block that loses one rule, or
    // whose rules are renumbered, still matches.
    for r in rows {
        let want_subset = match r.kind {
            Kind::Create => true,
            Kind::Remove => false,
            _ => continue,
        };
        let norm_text = |doc: &XmlDocument, src: &str| -> BTreeMap<String, usize> {
            let mut m = BTreeMap::new();
            for n in selected(doc.root(), r, this_sheet) {
                let tag = &src[open_tag(n)];
                // the rows narrowed like this one that also cover `n`
                let peers: Vec<&Allowed> = rows.iter().filter(|x| x.path == r.path && selected(doc.root(), x, this_sheet).iter().any(|m| std::ptr::eq(*m, n))).collect();
                let attrs: Vec<&str> = peers.iter().flat_map(|x| match x.kind {
                    Kind::EditAttrs(a) => a.to_vec(),
                    _ => Vec::new(),
                }).collect();
                let content_free = peers.iter().any(|x| matches!(x.kind, Kind::Content));
                let key = if content_free {
                    strip_attrs(tag, &attrs) // the content may change
                } else {
                    subtree_key(doc, src, n, r.path, rows, this_sheet)
                };
                *m.entry(key).or_insert(0) += 1;
            }
            m
        };
        let (a, b) = (norm_text(&din, input), norm_text(&dout, output));
        let (small, big) = if want_subset { (&a, &b) } else { (&b, &a) };
        for (k, n) in small {
            if big.get(k).copied().unwrap_or(0) < *n {
                return Err(format!("{:?} at {}: {k} {}", r.kind, r.path, if want_subset { "disappeared" } else { "appeared" }));
            }
        }
    }
    Ok((expand_empty(&residue(input, &mut cuts.0, &norms.0)), expand_empty(&residue(output, &mut cuts.1, &norms.1))))
}

/// Residue normal form: `<x a="1"/>` written as `<x a="1"></x>` (an element
/// whose last child was created or removed may differ only in this form),
/// and indentation between tags (whitespace-only runs with a line break)
/// dropped, since writers that keep a pretty-printed part indented add it
/// on either side of a new element.
fn expand_empty(src: &str) -> String {
    let dropped = drop_indentation(src);
    let src: &str = &dropped;
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(end) = rest.find("/>") {
        let head = &rest[..end];
        match head.rfind('<') {
            Some(lt) if !head[lt..].starts_with("<?") && !head[lt..].starts_with("<!") => {
                let name = head[lt + 1..].split([' ', '\t', '\r', '\n']).next().unwrap_or_default();
                out.push_str(head.trim_end());
                out.push_str("></");
                out.push_str(name);
                out.push('>');
            }
            _ => {
                out.push_str(head);
                out.push_str("/>");
            }
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

fn drop_indentation(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(gt) = rest.find('>') {
        out.push_str(&rest[..=gt]);
        rest = &rest[gt + 1..];
        let ws = rest.len() - rest.trim_start_matches([' ', '\t', '\r', '\n']).len();
        if ws > 0 && rest[ws..].starts_with('<') && rest[..ws].contains('\n') {
            rest = &rest[ws..];
        }
    }
    out.push_str(rest);
    out
}

/// The key of element `n` (at `path`) for the Create/Remove multisets.
fn subtree_key(doc: &XmlDocument, src: &str, n: &RawNode, path: &str, rows: &[Allowed], this_sheet: Option<&str>) -> String {
    let outer = whole(n);
    let inside = |m: &RawNode| {
        let w = whole(m);
        outer.start <= w.start && w.end <= outer.end
    };
    // attribute normalizations, merged per start tag
    let mut attrs: BTreeMap<usize, (Range<usize>, Vec<&'static str>)> = BTreeMap::new();
    let mut cut: Vec<Range<usize>> = Vec::new();
    let deeper = format!("{path}/");
    for x in rows {
        for m in selected(doc.root(), x, this_sheet).into_iter().filter(|m| inside(m)) {
            match x.kind {
                Kind::EditAttrs(a) => attrs.entry(open_tag(m).start).or_insert_with(|| (open_tag(m), Vec::new())).1.extend_from_slice(a),
                Kind::Create | Kind::Remove | Kind::Subtree if x.path.starts_with(&deeper) => cut.push(whole(m)),
                _ => {}
            }
        }
    }
    let mut edits: Vec<(Range<usize>, String)> = attrs.into_values().map(|(r, a)| {
        let t = strip_attrs(&src[r.clone()], &a);
        (r, t)
    }).collect();
    edits.retain(|(r, _)| !cut.iter().any(|c| c.start <= r.start && r.end <= c.end));
    edits.extend(cut.into_iter().map(|r| (r, String::new())));
    edits.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let mut out = String::new();
    let mut pos = outer.start;
    for (r, t) in edits {
        if r.start < pos {
            continue; // inside a cut already applied
        }
        out.push_str(&src[pos..r.start]);
        out.push_str(&t);
        pos = r.end;
    }
    out.push_str(&src[pos..outer.end]);
    // `<x a="1"/>` and `<x a="1"></x>` are the same element
    if close_tag(n).is_none()
        && let Some(open) = out.strip_suffix("/>")
    {
        let name = src[outer.start + 1..].split([' ', '\t', '\r', '\n', '/', '>']).next().unwrap_or_default();
        out = format!("{}></{name}>", open.trim_end());
    }
    out
}

/// Checks `output` against `input` under `rows`. `sheet_part` is the edited
/// worksheet; `cells` the cells a `Kind::Cells` row allows (A1 refs).
pub fn check(input: &[u8], output: &[u8], sheet_part: &str, rows: &[Allowed], cells: &[String]) -> Result<(), String> {
    let wb_part = "xl/workbook.xml";
    let names_in: Vec<String> = entries(input).into_iter().map(|e| e.0).collect();
    let names_out: Vec<String> = entries(output).into_iter().map(|e| e.0).collect();
    let calc_removal = rows.iter().any(|r| r.part == Part::CalcChainRemoval);
    let other_cells = rows.iter().any(|r| r.part == Part::OtherSheetCells);
    for n in &names_in {
        if !names_out.contains(n) && !(calc_removal && n.ends_with("calcChain.xml")) {
            return Err(format!("part {n} disappeared"));
        }
    }
    let rels_part = match sheet_part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{sheet_part}.rels"),
    };
    let rels_may_appear = rows.iter().any(|r| r.part == Part::SheetRels && matches!(r.kind, Kind::Create));
    for n in &names_out {
        if !names_in.contains(n) && !(rels_may_appear && n.eq_ignore_ascii_case(&rels_part)) {
            return Err(format!("part {n} appeared"));
        }
    }
    for n in &names_out {
        let b = part(output, n);
        let a = if names_in.contains(n) {
            part(input, n)
        } else {
            // a new .rels part: judged against its root with no children
            let text = String::from_utf8_lossy(&b).into_owned();
            let doc = XmlDocument::parse(text.as_bytes()).map_err(|e| e.to_string())?;
            let root = doc.root();
            let end = open_tag(root).end;
            let open = &text[..end];
            if open.ends_with("/>") {
                open.as_bytes().to_vec()
            } else {
                format!("{open}</{}>", text[open_tag(root).start + 1..].split([' ', '>', '/']).next().unwrap_or_default()).into_bytes()
            }
        };
        if a == b {
            continue;
        }
        let rows_here: Vec<Allowed> = rows
            .iter()
            .copied()
            .filter(|r| match r.part {
                // OPC part names compare case-insensitively (123233_charts.xlsx
                // names `Sheet1.xml`; the zip entry is `sheet1.xml`)
                Part::Sheet => n.eq_ignore_ascii_case(sheet_part),
                Part::SheetRels => n.eq_ignore_ascii_case(&rels_part),
                Part::Workbook => n == wb_part,
                Part::OtherSheetCells => n.starts_with("xl/worksheets/") && !n.contains("/_rels/") && !n.eq_ignore_ascii_case(sheet_part) && r.path == "sheetData",
                Part::CalcChainRemoval => false,
            })
            .collect();
        // a created sheet .rels part may be registered by core's content-type
        // rules (a `Default` for `rels`, or an `Override` for the part)
        let rels_created = rels_may_appear && !names_in.iter().any(|x| x.eq_ignore_ascii_case(&rels_part));
        let calc_side_effect = calc_removal && (n == "[Content_Types].xml" || n == "xl/_rels/workbook.xml.rels");
        let ct_side_effect = rels_created && n == "[Content_Types].xml";
        if calc_side_effect || ct_side_effect {
            let rels_name = format!("/{rels_part}").to_ascii_lowercase();
            let strip = |s: &str| {
                s.split('<')
                    .filter(|t| !(calc_side_effect && t.contains("calcChain")))
                    .filter(|t| !(ct_side_effect && (t.contains(r#"Extension="rels""#) || t.to_ascii_lowercase().contains(&format!(r#"partname="{rels_name}""#)))))
                    .collect::<Vec<_>>()
                    .join("<")
            };
            if strip(&drop_indentation(&String::from_utf8_lossy(&a))) != strip(&drop_indentation(&String::from_utf8_lossy(&b))) {
                return Err(format!("{n}: more than the calcChain or sheet .rels entries changed"));
            }
            continue;
        }
        if rows_here.is_empty() && !(other_cells && n.starts_with("xl/worksheets/")) {
            return Err(format!("part {n} changed, and no row allows it"));
        }
        let rows_here = if rows_here.is_empty() { vec![Allowed { part: Part::OtherSheetCells, path: "sheetData", kind: Kind::Cells, only: &[] }] } else { rows_here };
        let this_sheet = local_sheet_id(output, sheet_part);
        let (ra, rb) = judge(&String::from_utf8_lossy(&a), &String::from_utf8_lossy(&b), &rows_here, cells, this_sheet.as_deref())?;
        if ra != rb {
            let at = ra.bytes().zip(rb.bytes()).position(|(x, y)| x != y).unwrap_or(ra.len().min(rb.len()));
            let lo = at.saturating_sub(60);
            return Err(format!(
                "{n}: bytes outside the allowed changes differ at {at}:\n  in:  {:?}\n  out: {:?}",
                &ra[ra.floor_char_boundary(lo)..ra.floor_char_boundary((at + 60).min(ra.len()))],
                &rb[rb.floor_char_boundary(lo)..rb.floor_char_boundary((at + 60).min(rb.len()))]
            ));
        }
    }
    Ok(())
}
