//! Canonical cell form at save (ADR-0024 C7, §7.1).
//!
//! The text of a changed `c` depends on the history that led to it (F11): a
//! `set_style_index(0)` on a cell without `s` writes `s="0"` where a plain
//! `set_number` writes none, and a node that lost its children renders as
//! `<c …></c>`. [`Workbook::canonicalize_cells`] puts each changed cell in
//! one form whatever its history; the Journal's Commit calls it before
//! serialization.
//!
//! Visibility: `pub(crate)`, as the Journal (J0) is in this crate. ooxml-core
//! is not touched, so Layer 2's default rendering (docx, pptx) is unchanged.
//!
//! Which cells count as "changed", and what the Base node had: the caller
//! passes the changed set (ADR-0024 §4.4); dirty is not changed. A cell whose
//! node is not `subtree_dirty` is the Base's node (or restored to it) and is
//! left alone, which is only a filter: a Base cell that a structural op merely
//! shifted is dirty but not changed, and the caller must not pass it (its
//! `s="00"` would be rewritten to `s="0"`). For a changed cell, the Base node
//! is the node the cell was parsed from: it has a source span, and the Base's
//! `s` is read from the original open tag in the part's retained source text
//! (`XmlDocument::source`), which edits never alter. A node without a span
//! was created in the session: there is no Base node. This derives the
//! answer from the document itself; the Journal need not carry a flag.
//!
//! Precondition: `XmlDocument::source()` is the session's Base (epoch 0, or
//! the output of the previous save once C6 / the (A') reopen replaced it).
//! The part must never be re-parsed at a structural op or an epoch boundary
//! (§4.4: a structural op only closes the snapshot; nodes stay dirty and keep
//! their span). Re-parsing would make the open tag of an intermediate state
//! the "Base" and the rule would silently read the wrong `s`. The source is
//! replaced only after a save.
//!
//! Call it once per save, on the part as the session left it: a rebuilt
//! `c` is a created node with no Base, so a second call would see "no Base
//! `s`" for it (a rebuilt cell with `s="0"` stays `s="0"`; one whose `s` was
//! then set to 0 again would lose it). The Journal re-parses the part after
//! the save, which gives the next epoch its Base.

#![cfg_attr(not(test), allow(dead_code))] // the Journal (J0) is the first caller

use ooxml_core::{ElementSpec, RawItem, RawNode, XmlDocument};
use ooxml_schema::sml;

use crate::error::{Error, Result};
use crate::reference::CellIndex;
use crate::workbook::Workbook;

/// The attributes of a cell that a canonical rebuild can copy. A cell with
/// another attribute keeps its node (only `s` is normalized on it).
const COPIED: [ooxml_core::QName; 6] = [
    sml::Cell::ATTR_R,
    sml::Cell::ATTR_S,
    sml::Cell::ATTR_T,
    sml::Cell::ATTR_CM,
    sml::Cell::ATTR_VM,
    sml::Cell::ATTR_PH,
];

/// The value of the unprefixed attribute `s` in the open tag `tag`
/// (`<c r="A1" s="5">`), or `None` if the tag has none.
fn source_s(tag: &str) -> Option<String> {
    let b = tag.as_bytes();
    // skip `<` and the element name
    let mut i = 1;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'/' | b'>') {
            i += 1;
        }
        if name_start == i {
            return None;
        }
        let name = &tag[name_start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let quote = *b.get(i)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let v_start = i + 1;
        let v_len = tag[v_start..].find(quote as char)?;
        if name == "s" {
            return Some(tag[v_start..v_start + v_len].to_owned());
        }
        i = v_start + v_len + 1;
    }
}

/// Did the Base node of `cell` have an `s`? `None`: there is no Base node.
fn base_has_s(doc: &XmlDocument, cell: &RawNode) -> Option<bool> {
    let span = cell.span()?;
    let tag = doc.source().get(span.start..span.open_end)?;
    Some(source_s(tag).is_some())
}

/// Does `s` index `xf` 0? An absent `s` does.
fn is_xf0(s: Option<&str>) -> bool {
    s.is_none_or(|s| s.trim().parse::<u32>() == Ok(0))
}

impl Workbook {
    /// Writes each cell of `cells` on `sheet` that has changed in the
    /// canonical form (ADR-0024 C7):
    ///
    /// - an `s` that resolves to `xf` 0 (absent, or index 0) is absent when
    ///   the cell has no Base node or its Base node has no `s`, and `s="0"`
    ///   otherwise;
    /// - a `c` without children (whitespace-only text counts as none) is replaced by a fresh empty-element node
    ///   (own-dirty, no source span), which renders `<c …/>`.
    ///
    /// `cells` is the changed set the caller tracks (§4.4), not "every dirty
    /// cell". A cell that is missing, or whose node is clean (the Base's, or restored
    /// to it), is left untouched. A cell with an attribute other than
    /// `r`, `s`, `t`, `cm`, `vm`, `ph` gets the `s` rule only. Cells may come
    /// in any order and repeat. `Err` for a sheet whose cells cannot be
    /// located (the refusals of any cell edit); nothing changes then.
    pub(crate) fn canonicalize_cells(
        &mut self,
        sheet: usize,
        cells: impl IntoIterator<Item = CellIndex>,
    ) -> Result<()> {
        self.check_poisoned()?;
        self.ensure_index(sheet)?;
        let part = self.sheets[sheet]
            .part
            .clone()
            .ok_or_else(|| Error::Malformed("worksheet part".into()))?;
        // locate first, so that an `Err` changes nothing
        let mut paths = Vec::new();
        {
            let doc = self.pkg.xml_part(&part)?;
            let index = self.indexes.get(&sheet).expect("built by ensure_index");
            let sd = doc
                .root()
                .children()
                .iter()
                .position(|c| c.as_element().is_some_and(|e| e.is(sml::SheetData::QNAME)));
            for at in cells {
                let loc = crate::worksheet::locate(doc, index, &part, at)?;
                if let (Some(si), Some(ri), Some(ci)) = (sd, loc.row, loc.cell) {
                    paths.push([si, ri, ci]);
                }
            }
        }
        paths.sort_unstable();
        paths.dedup();
        let doc = self.pkg.xml_part_mut(&part)?;
        for path in paths {
            canonicalize_at(doc, path);
        }
        Ok(())
    }
}

/// What to do with one changed cell.
struct Plan {
    /// The target `s`: `None` = absent.
    s: Option<String>,
    /// Replace the node by a fresh empty element with these attributes (but
    /// `s`, which goes in after `r`).
    rebuild: Option<Vec<(ooxml_core::QName, String)>>,
}

fn plan(doc: &XmlDocument, path: &[usize; 3]) -> Option<Plan> {
    let cell = doc.root().children()[path[0]].as_element()?.children()[path[1]]
        .as_element()?
        .children()[path[2]]
        .as_element()?;
    if !cell.subtree_dirty() {
        return None;
    }
    let cur_s = cell.attr(sml::Cell::ATTR_S);
    let s = if is_xf0(cur_s) {
        (base_has_s(doc, cell) == Some(true)).then(|| "0".to_owned())
    } else {
        cur_s.map(str::to_owned)
    };
    let copyable = cell
        .attrs()
        .iter()
        .all(|a| COPIED.iter().any(|q| a.name().is(*q)));
    // whitespace-only text (a pretty-printed source) is no child
    let childless = cell
        .children()
        .iter()
        .all(|c| matches!(c, RawItem::Text(t) if t.value().trim().is_empty()));
    let rebuild = (childless && copyable).then(|| {
        cell.attrs()
            .iter()
            .filter_map(|a| {
                COPIED
                    .iter()
                    .find(|q| a.name().is(**q))
                    .map(|q| (*q, a.value().to_owned()))
            })
            .filter(|(q, _)| *q != sml::Cell::ATTR_S)
            .collect()
    });
    Some(Plan { s, rebuild })
}

/// Canonicalizes the `c` at `path` (sheetData, row, cell item indices).
/// Replacing keeps the item index, so the other paths stay valid.
fn canonicalize_at(doc: &mut XmlDocument, path: [usize; 3]) {
    let Some(Plan { s, rebuild }) = plan(doc, &path) else {
        return;
    };
    if let Some(attrs) = rebuild {
        // a fresh node goes in before the old one, which is removed after
        let mut row = doc
            .lens_mut_at::<sml::Row>(&path[..2])
            .expect("the located row");
        if let Ok(mut fresh) = row.insert_at::<sml::Cell>(path[2]) {
            // usual order: r s t cm vm ph
            let mut all = attrs.clone();
            if let Some(s) = &s {
                all.push((sml::Cell::ATTR_S, s.clone()));
            }
            all.sort_by_key(|(q, _)| COPIED.iter().position(|c| c == q));
            let ok = all.iter().all(|(q, v)| fresh.set_attr(*q, v).is_ok());
            if ok {
                row.remove_at(path[2] + 1);
                return;
            }
            row.remove_at(path[2]); // give the fresh node up
        }
    }
    set_s(doc, &path, s.as_deref());
}

/// Sets or removes `s` on the cell at `path`; writes nothing when it already is so.
fn set_s(doc: &mut XmlDocument, path: &[usize; 3], s: Option<&str>) {
    let mut cell = doc
        .lens_mut_at::<sml::Cell>(path)
        .expect("the located cell");
    let _ = match s {
        Some(s) => cell.set_attr(sml::Cell::ATTR_S, s),
        None => cell.remove_attr(sml::Cell::ATTR_S).map(|_| ()),
    };
}

#[cfg(test)]
mod tests {
    use ooxml_core::Package;

    use super::*;
    use crate::edit::CellOp;
    use crate::workbook::{CalcPolicy, TEMPLATE};

    const PART: &str = "xl/worksheets/sheet1.xml";
    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:F3"/><sheetData>"#;
    const TAIL: &str = "</sheetData></worksheet>";

    fn sheet_xml() -> String {
        let rows = concat!(
            r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1" s="0"><v>2</v></c><c r="C1" s="5"><v>3</v></c><c r="D1" t="n" s="05"><v>4</v></c><c r="E1"><v>5</v></c></row>"#,
            "\n",
            r#"<row r="2"><c r="A2" s="'x'"/></row>"#,
        );
        format!("{HEAD}{rows}{TAIL}")
    }

    fn open(xml: &str) -> Workbook {
        let mut pkg = Package::from_bytes(TEMPLATE).unwrap();
        pkg.set_part(PART, xml.as_bytes().to_vec()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        wb.policy = CalcPolicy::KeepStaleCaches;
        wb
    }

    fn at(a1: &str) -> CellIndex {
        CellIndex::parse(a1).unwrap()
    }

    fn part(wb: &mut Workbook) -> String {
        wb.pkg.xml_part(PART).unwrap().serialize()
    }

    /// `s` written by hand, as `set_style_index` leaves it.
    fn set_s(wb: &mut Workbook, path: [usize; 3], s: &str) {
        let doc = wb.pkg.xml_part_mut(PART).unwrap();
        doc.lens_mut_at::<sml::Cell>(&path)
            .unwrap()
            .set_attr(sml::Cell::ATTR_S, s)
            .unwrap();
    }

    fn canon(wb: &mut Workbook, cells: &[&str]) -> String {
        wb.canonicalize_cells(0, cells.iter().map(|c| at(c)))
            .unwrap();
        assert!(wb.pkg.xml_part(PART).unwrap().dirty_counts_consistent());
        part(wb)
    }

    #[test]
    fn source_s_reads_the_original_open_tag() {
        assert_eq!(source_s(r#"<c r="A1" s="5">"#).as_deref(), Some("5"));
        assert_eq!(source_s(r#"<c r="A1" s='7'/>"#).as_deref(), Some("7"));
        assert_eq!(source_s(r#"<c r="A1">"#), None);
        assert_eq!(source_s(r#"<c r="A1" t="s"/>"#), None);
        assert_eq!(source_s(r#"<x:c x:s="5" r="A1">"#), None);
        assert_eq!(
            source_s("<c\n  s = \"3\"\n r=\"A1\">").as_deref(),
            Some("3")
        );
    }

    #[test]
    fn xf0_on_a_cell_without_s_is_absent() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("A1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 0], "0");
        assert!(part(&mut wb).contains(r#"<c r="A1" s="0">"#));
        let out = canon(&mut wb, &["A1"]);
        assert!(out.contains(r#"<c r="A1"><v>9</v></c>"#), "{out}");
    }

    #[test]
    fn the_real_setter_agrees() {
        let mut wb = open(&sheet_xml());
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 4)
            .unwrap()
            .set_style_index(0)
            .unwrap();
        assert!(part(&mut wb).contains(r#"<c r="E1" s="0">"#));
        let out = canon(&mut wb, &["E1"]);
        assert!(out.contains(r#"<c r="E1"><v>5</v></c>"#), "{out}");
    }

    #[test]
    fn xf0_on_a_base_cell_with_s0_stays_s0() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("B1"), CellOp::Number("9".into()))
            .unwrap();
        let out = canon(&mut wb, &["B1"]);
        assert!(out.contains(r#"<c r="B1" s="0"><v>9</v></c>"#), "{out}");
        // the `s` a write dropped comes back as "0"
        let doc = wb.pkg.xml_part_mut(PART).unwrap();
        doc.lens_mut_at::<sml::Cell>(&[1, 0, 1])
            .unwrap()
            .remove_attr(sml::Cell::ATTR_S)
            .unwrap();
        let out = canon(&mut wb, &["B1"]);
        assert!(out.contains(r#"<c r="B1" s="0"><v>9</v></c>"#), "{out}");
    }

    #[test]
    fn s5_set_to_0_where_the_base_had_s5_becomes_s0() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("C1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 2], "0");
        let out = canon(&mut wb, &["C1"]);
        assert!(out.contains(r#"<c r="C1" s="0"><v>9</v></c>"#), "{out}");
        // another index than 0 is kept as written
        set_s(&mut wb, [1, 0, 2], "7");
        let out = canon(&mut wb, &["C1"]);
        assert!(out.contains(r#"<c r="C1" s="7"><v>9</v></c>"#), "{out}");
    }

    #[test]
    fn s_spelled_00_resolves_to_xf0() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("D1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 3], "00");
        let out = canon(&mut wb, &["D1"]);
        assert!(
            out.contains(r#"s="0""#) && !out.contains(r#"s="00""#),
            "{out}"
        );
    }

    #[test]
    fn a_new_cell_has_no_s() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("F1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 5], "0");
        let out = canon(&mut wb, &["F1"]);
        assert!(out.contains(r#"<c r="F1"><v>9</v></c>"#), "{out}");
        // also a new cell in a new row
        wb.edit_cell(0, at("B3"), CellOp::Number("1".into()))
            .unwrap();
        set_s(&mut wb, [1, 2, 0], "0");
        let out = canon(&mut wb, &["B3"]);
        assert!(out.contains(r#"<c r="B3"><v>1</v></c>"#), "{out}");
    }

    #[test]
    fn a_cell_that_lost_its_children_is_self_closing() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("C1"), CellOp::Clear).unwrap();
        let before = part(&mut wb);
        assert!(
            before.contains(r#"<c r="C1" s="5"></c>"#),
            "Layer 2 renders the emptied node with a close tag: {before}"
        );
        let out = canon(&mut wb, &["C1"]);
        assert!(out.contains(r#"<c r="C1" s="5"/>"#), "{out}");
        // the node is a fresh element, as the lens makes it
        let doc = wb.pkg.xml_part(PART).unwrap();
        let c = doc.root().children()[1].as_element().unwrap().children()[0]
            .as_element()
            .unwrap()
            .children()[2]
            .as_element()
            .unwrap();
        assert!(c.span().is_none() && c.own_dirty() && c.children().is_empty());
        // idempotent: the rebuilt node is already canonical
        let out = canon(&mut wb, &["C1"]);
        assert!(out.contains(r#"<c r="C1" s="5"/>"#), "idempotent: {out}");
        // the xf 0 rule applies on the rebuild: the Base had `s`, so `s="0"`
        let mut wb2 = open(&sheet_xml());
        wb2.edit_cell(0, at("C1"), CellOp::Clear).unwrap();
        set_s(&mut wb2, [1, 0, 2], "0");
        let out = canon(&mut wb2, &["C1"]);
        assert!(out.contains(r#"<c r="C1" s="0"/>"#), "{out}");
        // a cleared cell without a Base `s` and with xf 0
        wb.edit_cell(0, at("A1"), CellOp::Clear).unwrap();
        set_s(&mut wb, [1, 0, 0], "0");
        let out = canon(&mut wb, &["A1"]);
        assert!(out.contains(r#"<c r="A1"/>"#), "{out}");
    }

    #[test]
    fn the_other_cells_are_untouched_and_so_is_an_unchanged_workbook() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let all = ["A1", "B1", "C1", "D1", "E1", "A2", "Z9"];
        assert_eq!(canon(&mut wb, &all), xml);
        assert!(!wb.pkg.xml_part(PART).unwrap().is_dirty());
        // a change to C1 only: the rest of the part is byte for byte the source
        wb.edit_cell(0, at("C1"), CellOp::Number("9".into()))
            .unwrap();
        let before = part(&mut wb);
        let out = canon(&mut wb, &all);
        assert_eq!(out, before);
        assert_eq!(
            out.replace(
                r#"<c r="C1" s="5"><v>9</v></c>"#,
                r#"<c r="C1" s="5"><v>3</v></c>"#
            ),
            xml
        );
    }

    #[test]
    fn a_cell_not_listed_is_not_canonicalized() {
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("A1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 0], "0");
        let out = canon(&mut wb, &["E1"]);
        assert!(out.contains(r#"<c r="A1" s="0">"#), "{out}");
    }

    #[test]
    fn an_unlocatable_sheet_is_refused_and_unchanged() {
        let xml = sheet_xml().replace(r#"<c r="E1">"#, r#"<c r="A1">"#);
        let mut wb = open(&xml);
        assert!(wb.canonicalize_cells(0, [at("A1")]).is_err());
        assert_eq!(part(&mut wb), xml);
    }

    #[test]
    fn the_base_s_is_the_session_base_across_epochs() {
        // Base: A1 has no `s`. Epoch 0 writes s="7"; epoch 1 sets s="0".
        // The part is not re-parsed at the boundary (§4.4), so the Base tag
        // still says "no s" and the canonical form has none.
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("A1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 0], "7");
        wb.edit_cell(0, at("A1"), CellOp::Number("8".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 0], "0");
        let out = canon(&mut wb, &["A1"]);
        assert!(out.contains(r#"<c r="A1"><v>8</v></c>"#), "{out}");
        // Re-parsing at the boundary would make epoch 0's tag the "Base" and
        // read `s="7"`: wrong. This is why the precondition forbids it.
        let mut wb = open(&sheet_xml());
        wb.edit_cell(0, at("A1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 0], "7");
        let mid = part(&mut wb);
        let mut re = open(&mid);
        re.edit_cell(0, at("A1"), CellOp::Number("8".into()))
            .unwrap();
        set_s(&mut re, [1, 0, 0], "0");
        let out = canon(&mut re, &["A1"]);
        assert!(out.contains(r#"<c r="A1" s="0"><v>8</v></c>"#), "{out}");
    }

    #[test]
    fn a_pretty_printed_cell_that_lost_its_children_is_self_closing() {
        let xml = format!(
            "{HEAD}\n<row r=\"1\">\n  <c r=\"A1\" s=\"3\">\n    <v>1</v>\n  </c>\n  <c r=\"B1\">\n    <v>2</v>\n  </c>\n</row>\n{TAIL}"
        );
        let mut wb = open(&xml);
        wb.edit_cell(0, at("A1"), CellOp::Clear).unwrap();
        let out = canon(&mut wb, &["A1"]);
        assert!(out.contains(r#"<c r="A1" s="3"/>"#), "{out}");
        // B1 is unchanged: its bytes, whitespace included, stay
        assert!(
            out.contains("<c r=\"B1\">\n    <v>2</v>\n  </c>"),
            "{out}"
        );
    }

    #[test]
    fn a_rebuild_keeps_cm_vm_ph_in_the_usual_order() {
        let xml = format!(
            r#"{HEAD}<row r="1"><c r="A1" ph="1" vm="2" cm="3" t="n" s="4"><v>1</v></c></row>{TAIL}"#
        );
        let mut wb = open(&xml);
        // `edit_cell` refuses unknown metadata ids: drop the children by hand
        wb.pkg
            .xml_part_mut(PART)
            .unwrap()
            .lens_mut_at::<sml::Cell>(&[1, 0, 0])
            .unwrap()
            .remove_at(0);
        let out = canon(&mut wb, &["A1"]);
        assert!(
            out.contains(r#"<c r="A1" s="4" t="n" cm="3" vm="2" ph="1"/>"#),
            "{out}"
        );
    }

    #[test]
    fn a_cell_put_back_by_restore_cell_reads_the_base_s() {
        use crate::snapshot::RestoreStyle;
        let mut wb = open(&sheet_xml());
        // A1: Base has no s. B1: Base has s="0".
        wb.edit_cell(0, at("A1"), CellOp::Number("9".into()))
            .unwrap();
        wb.edit_cell(0, at("B1"), CellOp::Number("9".into()))
            .unwrap();
        set_s(&mut wb, [1, 0, 0], "0");
        wb.pkg
            .xml_part_mut(PART)
            .unwrap()
            .lens_mut_at::<sml::Cell>(&[1, 0, 1])
            .unwrap()
            .remove_attr(sml::Cell::ATTR_S)
            .unwrap();
        // a later epoch's snapshots: dirty nodes that keep their span
        let a = wb.snapshot_cell(0, at("A1")).unwrap();
        let b = wb.snapshot_cell(0, at("B1")).unwrap();
        wb.edit_cell(0, at("A1"), CellOp::Number("8".into()))
            .unwrap();
        wb.edit_cell(0, at("B1"), CellOp::Number("8".into()))
            .unwrap();
        wb.restore_cell(0, at("A1"), &a, RestoreStyle::Snapshot)
            .unwrap();
        wb.restore_cell(0, at("B1"), &b, RestoreStyle::Snapshot)
            .unwrap();
        let out = canon(&mut wb, &["A1", "B1"]);
        assert!(out.contains(r#"<c r="A1"><v>9</v></c>"#), "{out}");
        assert!(out.contains(r#"<c r="B1" s="0"><v>9</v></c>"#), "{out}");
    }

    #[test]
    fn a_base_cell_shifted_by_a_structural_op_keeps_its_base_s() {
        let mut wb = open(&sheet_xml());
        wb.worksheet().unwrap().insert_rows(0, 1).unwrap();
        // B1 (Base s="0") is now B2; its node is dirty with its old span
        wb.edit_cell(0, at("B2"), CellOp::Number("9".into()))
            .unwrap();
        let out = canon(&mut wb, &["B2"]);
        assert!(out.contains(r#"<c r="B2" s="0"><v>9</v></c>"#), "{out}");
        // a changed Base cell without s stays without it
        wb.edit_cell(0, at("A2"), CellOp::Number("9".into()))
            .unwrap();
        let doc = wb.pkg.xml_part_mut(PART).unwrap();
        doc.lens_mut_at::<sml::Cell>(&[1, 0, 0])
            .unwrap()
            .set_attr(sml::Cell::ATTR_S, "0")
            .unwrap();
        let out = canon(&mut wb, &["A2"]);
        assert!(out.contains(r#"<c r="A2"><v>9</v></c>"#), "{out}");
    }
}
