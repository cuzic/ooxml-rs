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

/// A start tag's range and its normalized text.
type Norm = (Range<usize>, String);

/// One allowed change.
#[derive(Clone, Copy, Debug)]
pub struct Allowed {
    pub part: Part,
    pub path: &'static str,
    pub kind: Kind,
}

pub const fn sheet(path: &'static str, kind: Kind) -> Allowed {
    Allowed { part: Part::Sheet, path, kind }
}

pub const fn workbook(path: &'static str, kind: Kind) -> Allowed {
    Allowed { part: Part::Workbook, path, kind }
}

fn at_path<'n>(root: &'n RawNode, path: &str) -> Vec<&'n RawNode> {
    let mut cur = vec![root];
    for seg in path.split('/') {
        cur = cur.iter().flat_map(|n| n.elements().filter(|e| e.name().local() == seg)).collect();
    }
    cur
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
fn judge(input: &str, output: &str, rows: &[Allowed], cells: &[String]) -> Result<(String, String), String> {
    let din = XmlDocument::parse(input.as_bytes()).map_err(|e| e.to_string())?;
    let dout = XmlDocument::parse(output.as_bytes()).map_err(|e| e.to_string())?;
    let mut cuts = (Vec::new(), Vec::new());
    let mut norms: (Vec<Norm>, Vec<Norm>) = (Vec::new(), Vec::new());
    // attribute normalization, by path
    let attrs_at = |path: &str| -> Vec<&'static str> {
        rows.iter()
            .filter(|r| r.path == path)
            .flat_map(|r| match r.kind {
                Kind::EditAttrs(a) => a.to_vec(),
                _ => Vec::new(),
            })
            .collect()
    };
    for (doc, src, cut, norm) in [(&din, input, &mut cuts.0, &mut norms.0), (&dout, output, &mut cuts.1, &mut norms.1)] {
        for r in rows {
            for n in at_path(doc.root(), r.path) {
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
    // Create / Remove: compare the elements' texts (after attribute
    // normalization of the same path) as multisets
    for r in rows {
        let want_subset = match r.kind {
            Kind::Create => true,
            Kind::Remove => false,
            _ => continue,
        };
        let norm_text = |doc: &XmlDocument, src: &str| -> BTreeMap<String, usize> {
            let mut m = BTreeMap::new();
            for n in at_path(doc.root(), r.path) {
                let t = &src[whole(n)];
                let tag = &src[open_tag(n)];
                let content_free = rows.iter().any(|x| x.path == r.path && matches!(x.kind, Kind::Content));
                let key = if content_free {
                    strip_attrs(tag, &attrs_at(r.path)) // the content may change
                } else {
                    t.replacen(tag, &strip_attrs(tag, &attrs_at(r.path)), 1)
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
    Ok((residue(input, &mut cuts.0, &norms.0), residue(output, &mut cuts.1, &norms.1)))
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
    for n in &names_out {
        if !names_in.contains(n) {
            return Err(format!("part {n} appeared"));
        }
    }
    for n in &names_out {
        let (a, b) = (part(input, n), part(output, n));
        if a == b {
            continue;
        }
        let rows_here: Vec<Allowed> = rows
            .iter()
            .copied()
            .filter(|r| match r.part {
                Part::Sheet => n == sheet_part,
                Part::Workbook => n == wb_part,
                Part::OtherSheetCells => n.starts_with("xl/worksheets/") && n != sheet_part && r.path == "sheetData",
                Part::CalcChainRemoval => false,
            })
            .collect();
        let calc_side_effect = calc_removal && (n == "[Content_Types].xml" || n == "xl/_rels/workbook.xml.rels");
        if calc_side_effect {
            // the calcChain entries may go; nothing else
            let strip = |s: &str| s.split('<').filter(|t| !t.contains("calcChain")).collect::<Vec<_>>().join("<");
            if strip(&String::from_utf8_lossy(&a)) != strip(&String::from_utf8_lossy(&b)) {
                return Err(format!("{n}: more than the calcChain entry changed"));
            }
            continue;
        }
        if rows_here.is_empty() && !(other_cells && n.starts_with("xl/worksheets/")) {
            return Err(format!("part {n} changed, and no row allows it"));
        }
        let rows_here = if rows_here.is_empty() { vec![Allowed { part: Part::OtherSheetCells, path: "sheetData", kind: Kind::Cells }] } else { rows_here };
        let (ra, rb) = judge(&String::from_utf8_lossy(&a), &String::from_utf8_lossy(&b), &rows_here, cells)?;
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
