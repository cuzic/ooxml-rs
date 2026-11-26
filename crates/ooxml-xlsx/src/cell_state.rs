//! Cell state reads (ADR-0024 C8): a cell's `f` element as a typed value and
//! its rich runs, those of a shared rich string included. The public cell API
//! exposes neither (`CellRef::formula` is the text only; `CellRef::value` has
//! no runs), and §4.4's state equality and R1's projection P need both.
//!
//! Read-only: nothing here dirties a node or changes a byte of output.
//! Visibility: `pub(crate)`, the Journal (J0) lives in this crate; no new
//! public API (the existing public read API has no natural place for these).

#![cfg_attr(not(test), allow(dead_code))] // the Journal (J0) is the first caller

use std::fmt::Write as _;

use ooxml_core::{ElementSpec, RawItem, RawNode, XmlDocument};
use ooxml_schema::sml;

use crate::error::{Error, Result};
use crate::reference::CellIndex;
use crate::workbook::{REL_SHARED_STRINGS, Workbook};

/// A cell's `f` element. Equality is value equality: `ca="1"` and
/// `ca="true"` are the same, an absent boolean is `false`, an absent `t` is
/// `normal`. Attributes the schema does not list for `f` (and a boolean whose
/// value does not parse) are kept in [`other`](Self::other), so two formulas
/// that differ in one still differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormulaState {
    /// The element text, as written (shared members: usually empty).
    pub(crate) text: String,
    /// `t`: `normal`, `array`, `dataTable`, `shared`, or whatever was written.
    pub(crate) t: String,
    /// `ref`.
    pub(crate) reference: Option<String>,
    /// `si`, as written (a number in a valid file).
    pub(crate) si: Option<String>,
    pub(crate) ca: bool,
    pub(crate) aca: bool,
    pub(crate) dt2d: bool,
    pub(crate) dtr: bool,
    pub(crate) del1: bool,
    pub(crate) del2: bool,
    pub(crate) bx: bool,
    pub(crate) r1: Option<String>,
    pub(crate) r2: Option<String>,
    /// Every other attribute (`xml:space` included) as `(namespace, local,
    /// value)`, sorted, so that attribute order does not matter. `xmlns`
    /// declarations are not attributes of the formula and are left out.
    pub(crate) other: Vec<(String, String, String)>,
}

/// One run of a rich string: its text and its run properties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RichRun {
    /// The run's `t` text as written (not trimmed, `_xHHHH_` not decoded).
    /// Not the display text and not what the value API returns: compare it
    /// together with the cell value, never alone.
    pub(crate) text: String,
    /// The run's `rPr` in a canonical form (see [`canonical`]); `None` when
    /// the run has no `rPr`. A run with an empty `<rPr/>` differs from one
    /// without, as the file does.
    pub(crate) props: Option<String>,
}

/// The state of a string cell beyond its value: rich runs and phonetic data.
/// Two strings with the same text but different phonetics (`rPh`,
/// `phoneticPr`) are different states, as the file differs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StringState {
    /// The `r` runs; `None` for a plain string (no `r` child).
    pub(crate) runs: Option<Vec<RichRun>>,
    /// The `rPh` and `phoneticPr` children of the item in canonical form
    /// (see [`canonical`]), in document order, concatenated; `None` when it
    /// has neither.
    pub(crate) phonetic: Option<String>,
}

/// Canonical text of an element, for equality: names by namespace URI (not
/// prefix), attributes sorted, `xmlns` declarations dropped, whitespace-only
/// text between elements dropped, children in document order (the order of
/// `rPr` children is kept: a different order is a different state, which only
/// costs a restore that was not needed).
pub(crate) fn canonical(n: &RawNode) -> String {
    let mut out = String::new();
    write_canonical(n, &mut out);
    out
}

fn write_canonical(n: &RawNode, out: &mut String) {
    let _ = write!(out, "<{{{}}}{}", n.name().ns(), n.name().local());
    let mut attrs: Vec<_> = n.attrs().iter().filter(|a| !a.name().is_namespace_declaration()).collect();
    attrs.sort_by(|a, b| (a.name().ns(), a.name().local()).cmp(&(b.name().ns(), b.name().local())));
    for a in attrs {
        let _ = write!(out, " {{{}}}{}={:?}", a.name().ns(), a.name().local(), a.value());
    }
    out.push('>');
    let has_elements = n.elements().next().is_some();
    for item in n.children() {
        match item {
            RawItem::Element(c) => write_canonical(c, out),
            RawItem::Text(t) if !has_elements || !t.value().trim().is_empty() => {
                let _ = write!(out, "{:?}", t.value());
            }
            _ => {}
        }
    }
    out.push_str("</>");
}

impl FormulaState {
    /// The state of the `f` child of a `c` node (live or a snapshot's);
    /// `None` when it has none.
    pub(crate) fn of(c: &RawNode) -> Option<FormulaState> {
        c.elements().find(|e| e.is(sml::CellFormula::QNAME)).map(formula_state)
    }
}

fn formula_state(f: &RawNode) -> FormulaState {
    let mut s = FormulaState {
        text: f.text().into_owned(),
        t: "normal".into(),
        reference: None,
        si: None,
        ca: false,
        aca: false,
        dt2d: false,
        dtr: false,
        del1: false,
        del2: false,
        bx: false,
        r1: None,
        r2: None,
        other: Vec::new(),
    };
    for a in f.attrs() {
        let name = a.name();
        if name.is_namespace_declaration() {
            continue;
        }
        let v = a.value();
        let flag = |slot: &mut bool| match ooxml_core::value::parse_bool(v) {
            Some(b) => {
                *slot = b;
                true
            }
            None => false,
        };
        let taken = if name.ns().is_empty() {
            match name.local() {
                "t" => {
                    s.t = v.to_owned();
                    true
                }
                "ref" => {
                    s.reference = Some(v.to_owned());
                    true
                }
                "si" => {
                    s.si = Some(v.to_owned());
                    true
                }
                "r1" => {
                    s.r1 = Some(v.to_owned());
                    true
                }
                "r2" => {
                    s.r2 = Some(v.to_owned());
                    true
                }
                "ca" => flag(&mut s.ca),
                "aca" => flag(&mut s.aca),
                "dt2D" => flag(&mut s.dt2d),
                "dtr" => flag(&mut s.dtr),
                "del1" => flag(&mut s.del1),
                "del2" => flag(&mut s.del2),
                "bx" => flag(&mut s.bx),
                _ => false,
            }
        } else {
            false
        };
        if !taken {
            s.other.push((name.ns().to_owned(), name.local().to_owned(), v.to_owned()));
        }
    }
    s.other.sort();
    s
}

/// The string state of an `is` or `si`. `runs` is `None` when it has no `r`
/// child (a plain string, whatever its `t`); `phonetic` is `None` when it has
/// neither `rPh` nor `phoneticPr`.
fn string_state_of_item(item: &RawNode) -> StringState {
    let runs: Vec<RichRun> = item
        .elements()
        .filter(|c| c.is(sml::Run::QNAME))
        .map(|r| RichRun {
            text: r.elements().filter(|c| c.is(sml::Text::QNAME)).map(|t| t.text()).collect::<String>(),
            props: r.elements().find(|c| c.is(sml::RunProperties::QNAME)).map(canonical),
        })
        .collect();
    let ns = item.name().ns();
    let phonetic: String = item
        .elements()
        .filter(|c| c.name().ns() == ns && matches!(c.name().local(), "rPh" | "phoneticPr"))
        .map(canonical)
        .collect();
    StringState { runs: (!runs.is_empty()).then_some(runs), phonetic: (!phonetic.is_empty()).then_some(phonetic) }
}

/// The [`StringState`] a rich-string write leaves: `runs` built as the
/// write builds them (no phonetics, no extras), so a same-runs write
/// compares equal to the current inline state. C1's report compares it
/// against the live cell (single source: the write's own builder).
#[cfg_attr(not(test), expect(dead_code))] // slice A: unit tests only; slice B wires the Journal
pub(crate) fn rich_state_of_runs(
    runs: &[crate::registry::strings::RichRun],
) -> Result<StringState> {
    let mut doc = XmlDocument::parse(format!(r#"<is xmlns="{}"/>"#, crate::SML_NS).as_bytes())?;
    {
        let mut is = doc.lens_mut::<sml::InlineString>()?;
        for r in runs {
            crate::registry::strings::build_run(&mut is, r)?;
        }
    }
    Ok(string_state_of_item(doc.root()))
}

impl Workbook {
    /// Runs `read` on the `c` node at `sheet!at`; `None` when the cell is
    /// missing. Takes `&mut self` like every cell lookup (the sheet's row
    /// index and the part's tree are built on first use); it changes no
    /// node and no output byte.
    fn with_cell<T>(&mut self, sheet: usize, at: CellIndex, read: impl FnOnce(&RawNode) -> T) -> Result<Option<T>> {
        let (part, spot) = self.cell_spot(sheet, at)?;
        let doc = self.pkg.xml_part(&part)?;
        let sd = spot.sheet_data.and_then(|i| doc.root().children()[i].as_element());
        let row = sd.zip(spot.row).and_then(|(sd, i)| sd.children()[i].as_element());
        let cell = row.zip(spot.cell).and_then(|(r, i)| r.children()[i].as_element());
        Ok(cell.map(read))
    }

    /// The cell's `f` element as a typed value; `None` for a missing cell or
    /// a cell without a formula. For a snapshot's node use
    /// [`FormulaState::of`].
    pub(crate) fn cell_formula_state(&mut self, sheet: usize, at: CellIndex) -> Result<Option<FormulaState>> {
        let r = self.with_cell(sheet, at, FormulaState::of)?;
        Ok(r.flatten())
    }

    /// [`string_state_of`](Self::string_state_of) for the live cell at
    /// `sheet!at`; `None` for a missing cell.
    pub(crate) fn cell_string_state(&mut self, sheet: usize, at: CellIndex) -> Result<Option<StringState>> {
        // a clone of one `c`, so the shared-string table can be read next
        let Some(c) = self.with_cell(sheet, at, RawNode::clone)? else { return Ok(None) };
        self.string_state_of(&c, at)
    }

    /// The string state of a `c` node, live or a snapshot's
    /// (`CellSnapshot::node`): that of its `is` (`t="inlineStr"`) or of the
    /// shared-string item its `v` points at (`t="s"`), resolved against the
    /// workbook's current table. `at` only names the cell in errors.
    ///
    /// `None` when the node is not a string cell, or is `t="s"` without a
    /// `v` (the value API reads that as empty, so does this).
    /// `Err` for a shared-string index that resolves to nothing, or a table
    /// whose items sit behind `mc:AlternateContent` (the index is then
    /// reader-dependent, as for the registry); a caller comparing states
    /// should treat `Err` as "unequal" (the cost is one needless restore).
    pub(crate) fn string_state_of(&mut self, c: &RawNode, at: CellIndex) -> Result<Option<StringState>> {
        let v = match c.attr(sml::Cell::ATTR_T) {
            Some("inlineStr") => {
                return Ok(c.elements().find(|e| e.is(sml::InlineString::QNAME)).map(string_state_of_item));
            }
            Some("s") => match c.elements().find(|e| e.is(sml::CellValue::QNAME)) {
                Some(v) => v.text().into_owned(),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        let bad = |m: String| Error::InvalidValue { cell: at.to_string(), message: m };
        let i = ooxml_core::value::parse_u32(&v).ok_or_else(|| bad(format!("shared-string index {v:?}")))? as usize;
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let part = rels
            .by_type(REL_SHARED_STRINGS)
            .next()
            .and_then(|r| rels.resolve(r))
            .filter(|p| self.pkg.contains(p))
            .ok_or_else(|| bad("no shared-string table".into()))?;
        let root = self.pkg.xml_part(&part)?.root();
        if root.elements().any(|c| c.is(ooxml_core::name::MC_ALTERNATE_CONTENT)) {
            return Err(ooxml_core::Error::UnresolvedSibling(sml::SharedStringTable::QNAME).into());
        }
        let si = root
            .elements()
            .filter(|c| c.is(sml::SharedStringItem::QNAME))
            .nth(i)
            .ok_or_else(|| bad(format!("shared-string index {i} out of range")))?;
        Ok(Some(string_state_of_item(si)))
    }
}

#[cfg(test)]
mod tests {
    use ooxml_core::Package;

    use super::*;
    use crate::edit::CellOp;
    use crate::workbook::TEMPLATE;

    const SHEET: &str = "xl/worksheets/sheet1.xml";
    const SST: &str = "xl/sharedStrings.xml";
    const BOLD: &str = r#"<rPr><b/><sz val="11"/><color rgb="FFFF0000"/><rFont val="Calibri"/></rPr>"#;

    fn sheet_xml() -> String {
        let rows = [
            // A1 shared rich string (si 1), A2 plain shared (si 0), A3 inline rich, A4 inline plain
            r#"<row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="s"><v>0</v></c></row>"#,
            r#"<row r="2"><c r="A2" t="inlineStr"><is><r><rPr><i/></rPr><t xml:space="preserve">a </t></r><r><t>b</t></r></is></c><c r="B2" t="inlineStr"><is><t>plain</t></is></c></row>"#,
            r#"<row r="3"><c r="A3"><f>1+1</f><v>2</v></c><c r="B3"><f t="shared" ref="B3:B4" si="0" ca="1">A1*2</f><v>2</v></c><c r="C3"><f t="array" ref="C3:D3" aca="true" xml:space="preserve">{1,2}</f><v>1</v></c></row>"#,
            r#"<row r="4"><c r="B4"><f t="shared" si="0"/><v>4</v></c><c r="C4"><f t="dataTable" ref="C4:C5" dt2D="1" dtr="0" r1="A1" r2="A2" del1="1" bx="1" foo="bar"/></c><c r="D4"><v>5</v></c></row>"#,
        ];
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{}</sheetData></worksheet>"#,
            rows.join("\n")
        )
    }

    fn sst_xml(prefix: &str) -> String {
        // same `rPr` as BOLD in the first run; the second run has none; a prefixed copy shares the namespace
        let (p, ns) = if prefix.is_empty() { (String::new(), r#" xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#.to_owned()) } else { (format!("{prefix}:"), format!(r#" xmlns:{prefix}="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#)) };
        let bold = BOLD.replace('<', &format!("<{p}")).replace(&format!("<{p}/"), &format!("</{p}"));
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<{p}sst{ns} count="2" uniqueCount="2"><{p}si><{p}t>zero</{p}t></{p}si><{p}si><{p}r>{bold}<{p}t>Bo</{p}t></{p}r><{p}r><{p}t xml:space="preserve">ld </{p}t></{p}r></{p}si></{p}sst>"#
        )
    }

    fn open_with(sheet: &str, sst: &str) -> (Workbook, Vec<u8>) {
        let mut pkg = Package::from_bytes(TEMPLATE).unwrap();
        pkg.set_part(SHEET, sheet.as_bytes().to_vec()).unwrap();
        pkg.set_part(SST, sst.as_bytes().to_vec()).unwrap();
        let bytes = pkg.to_bytes().unwrap();
        (Workbook::from_bytes(bytes.clone()).unwrap(), bytes)
    }

    fn open() -> (Workbook, Vec<u8>) {
        open_with(&sheet_xml(), &sst_xml(""))
    }

    fn runs(wb: &mut Workbook, a1: &str) -> Option<Vec<RichRun>> {
        wb.cell_string_state(0, at(a1)).unwrap().and_then(|s| s.runs)
    }

    fn at(a1: &str) -> CellIndex {
        CellIndex::parse(a1).unwrap()
    }

    #[test]
    fn the_template_has_a_shared_string_part() {
        // guards the fixture: the part the test overwrites is the one the workbook uses
        let (mut wb, _) = open();
        assert!(wb.shared_strings_active().unwrap());
        assert_eq!(wb.shared_strings.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn a_shared_rich_string_gives_its_runs() {
        let (mut wb, _) = open();
        let runs = runs(&mut wb, "A1").expect("rich");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "Bo");
        assert_eq!(runs[1].text, "ld ", "not trimmed");
        assert!(runs[0].props.as_deref().unwrap().contains("{http://schemas.openxmlformats.org/spreadsheetml/2006/main}b"));
        assert!(runs[0].props.as_deref().unwrap().contains(r#"rgb="FFFF0000""#));
        assert_eq!(runs[1].props, None);
    }

    #[test]
    fn a_plain_shared_string_and_a_plain_inline_string_have_no_runs() {
        let (mut wb, _) = open();
        for a1 in ["B1", "B2"] {
            assert_eq!(wb.cell_string_state(0, at(a1)).unwrap(), Some(StringState::default()), "{a1}");
        }
        // not string cells, and a missing cell
        for a1 in ["D4", "A3", "F9"] {
            assert_eq!(wb.cell_string_state(0, at(a1)).unwrap(), None, "{a1}");
        }
    }

    #[test]
    fn an_inline_rich_string_gives_its_runs() {
        let (mut wb, _) = open();
        let a2 = runs(&mut wb, "A2").expect("rich");
        assert_eq!(a2.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["a ", "b"]);
        assert!(a2[0].props.is_some() && a2[1].props.is_none());
        assert_ne!(a2[0].props, runs(&mut wb, "A1").unwrap()[0].props, "italic vs bold");
    }

    #[test]
    fn run_properties_compare_by_namespace_not_prefix_and_not_attribute_order() {
        let (mut wb1, _) = open();
        let (mut wb2, _) = open_with(&sheet_xml(), &sst_xml("x"));
        assert_eq!(runs(&mut wb1, "A1"), runs(&mut wb2, "A1"));
        let sst = sst_xml("").replace(r#"<color rgb="FFFF0000"/>"#, r#"<color rgb="FFFF0000" theme="1"/>"#);
        let (mut a, _) = open_with(&sheet_xml(), &sst);
        let sst2 = sst_xml("").replace(r#"<color rgb="FFFF0000"/>"#, r#"<color theme="1" rgb="FFFF0000"/>"#);
        let (mut b, _) = open_with(&sheet_xml(), &sst2);
        assert_eq!(runs(&mut a, "A1"), runs(&mut b, "A1"));
        assert_ne!(runs(&mut a, "A1"), runs(&mut wb1, "A1"));
    }

    #[test]
    fn a_dangling_shared_index_is_an_error() {
        let sheet = sheet_xml().replace("<v>1</v></c><c r=\"B1\"", "<v>9</v></c><c r=\"B1\"");
        let (mut wb, _) = open_with(&sheet, &sst_xml(""));
        assert!(wb.cell_string_state(0, at("A1")).is_err());
    }

    fn state(wb: &mut Workbook, a1: &str) -> FormulaState {
        wb.cell_formula_state(0, at(a1)).unwrap().expect(a1)
    }

    #[test]
    fn formula_attributes_are_typed() {
        let (mut wb, _) = open();
        let a3 = state(&mut wb, "A3");
        assert_eq!((a3.text.as_str(), a3.t.as_str(), a3.reference.as_deref(), a3.si.as_deref(), a3.ca), ("1+1", "normal", None, None, false));
        assert!(a3.other.is_empty());

        let b3 = state(&mut wb, "B3");
        assert_eq!((b3.t.as_str(), b3.reference.as_deref(), b3.si.as_deref(), b3.ca, b3.aca), ("shared", Some("B3:B4"), Some("0"), true, false));
        assert_eq!(b3.text, "A1*2");

        let b4 = state(&mut wb, "B4");
        assert_eq!((b4.t.as_str(), b4.si.as_deref(), b4.text.as_str(), b4.reference.as_deref()), ("shared", Some("0"), "", None));

        let c3 = state(&mut wb, "C3");
        assert_eq!((c3.t.as_str(), c3.reference.as_deref(), c3.aca, c3.ca, c3.text.as_str()), ("array", Some("C3:D3"), true, false, "{1,2}"));
        assert_eq!(c3.other, [("http://www.w3.org/XML/1998/namespace".to_owned(), "space".to_owned(), "preserve".to_owned())]);

        let c4 = state(&mut wb, "C4");
        assert_eq!(c4.t, "dataTable");
        assert_eq!((c4.dt2d, c4.dtr, c4.del1, c4.del2, c4.bx), (true, false, true, false, true));
        assert_eq!((c4.r1.as_deref(), c4.r2.as_deref()), (Some("A1"), Some("A2")));
        assert_eq!(c4.other, [(String::new(), "foo".to_owned(), "bar".to_owned())], "unknown attributes are kept");

        assert_eq!(wb.cell_formula_state(0, at("D4")).unwrap(), None, "a value cell");
        assert_eq!(wb.cell_formula_state(0, at("F9")).unwrap(), None, "a missing cell");
    }

    #[test]
    fn boolean_spellings_and_attribute_order_do_not_matter_but_values_do() {
        let mk = |f: &str| {
            let sheet = sheet_xml().replace(r#"<f>1+1</f>"#, f);
            let (mut wb, _) = open_with(&sheet, &sst_xml(""));
            state(&mut wb, "A3")
        };
        assert_eq!(mk(r#"<f ca="1" t="normal">1+1</f>"#), mk(r#"<f t="normal" ca="true">1+1</f>"#));
        assert_eq!(mk(r#"<f>1+1</f>"#), mk(r#"<f ca="0">1+1</f>"#));
        assert_ne!(mk(r#"<f>1+1</f>"#), mk(r#"<f ca="1">1+1</f>"#));
        assert_ne!(mk(r#"<f>1+1</f>"#), mk(r#"<f>1+2</f>"#));
        assert_ne!(mk(r#"<f>1+1</f>"#), mk(r#"<f x="1">1+1</f>"#));
        assert_eq!(mk(r#"<f x="1" y="2">1+1</f>"#), mk(r#"<f y="2" x="1">1+1</f>"#), "unknown attributes: order is not state");
        // an unparseable boolean is kept, not dropped
        assert_ne!(mk(r#"<f>1+1</f>"#), mk(r#"<f ca="maybe">1+1</f>"#));
    }

    #[test]
    fn reads_change_nothing() {
        let (mut wb, bytes) = open();
        for a1 in ["A1", "B1", "A2", "B2", "A3", "B3", "C3", "B4", "C4", "D4", "F9"] {
            wb.cell_formula_state(0, at(a1)).unwrap();
            let _ = wb.cell_string_state(0, at(a1)).unwrap();
        }
        for part in [SHEET, SST] {
            let doc = wb.pkg.xml_part(part).unwrap();
            assert!(!doc.is_dirty(), "{part}");
            assert!(doc.dirty_counts_consistent());
        }
        assert_eq!(wb.to_bytes().unwrap(), bytes, "the output is the input, byte for byte");
    }

    const PH: &str = r#"<rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="1"/>"#;

    /// sst with extra items: 2 plain + phonetics, 3 plain, 4 rich + phonetics,
    /// 5 same rich without, 6 plain + `phoneticPr` only.
    fn phonetic_workbook() -> Workbook {
        let items = format!(
            r#"<si><t>漢字</t>{PH}</si><si><t>漢字</t></si><si><r><rPr><b/></rPr><t>漢</t></r><r><t>字</t></r>{PH}</si><si><r><rPr><b/></rPr><t>漢</t></r><r><t>字</t></r></si><si><t>漢字</t><phoneticPr fontId="1"/></si>"#
        );
        let sst = sst_xml("").replace("</sst>", &format!("{items}</sst>"));
        let cells = r#"<c r="B1" t="s"><v>0</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c><c r="E1" t="s"><v>4</v></c><c r="F1" t="s"><v>5</v></c><c r="G1" t="s"><v>6</v></c><c r="H1" t="inlineStr"><is><t>漢字</t></is></c><c r="I1" t="inlineStr"><is><t>漢字</t>PHX</is></c><c r="J1" t="s"/>"#.replace("PHX", PH);
        let sheet = sheet_xml().replace(r#"<c r="B1" t="s"><v>0</v></c>"#, &cells);
        open_with(&sheet, &sst).0
    }

    #[test]
    fn phonetic_data_is_part_of_the_string_state() {
        let mut wb = phonetic_workbook();
        let get = |wb: &mut Workbook, a1: &str| wb.cell_string_state(0, at(a1)).unwrap().expect(a1);
        let (plain_ph, plain, rich_ph, rich, pr_only) = (get(&mut wb, "C1"), get(&mut wb, "D1"), get(&mut wb, "E1"), get(&mut wb, "F1"), get(&mut wb, "G1"));
        assert_ne!(plain_ph, plain, "same text, only the phonetics differ");
        assert_ne!(rich_ph, rich);
        assert_ne!(pr_only, plain, "phoneticPr alone counts");
        assert_ne!(pr_only, plain_ph);
        assert_eq!(plain, StringState::default());
        assert!(plain_ph.runs.is_none() && plain_ph.phonetic.as_deref().unwrap().contains("カンジ"));
        assert_eq!(rich_ph.runs, rich.runs, "the runs alone are equal");
        assert!(rich_ph.phonetic.is_some() && rich.phonetic.is_none());
        // inline strings too
        let (inline_plain, inline_ph) = (get(&mut wb, "H1"), get(&mut wb, "I1"));
        assert_ne!(inline_plain, inline_ph);
        assert_eq!(inline_ph.phonetic, plain_ph.phonetic, "inline and shared agree on the same phonetics");
    }

    #[test]
    fn a_shared_string_cell_without_v_reads_as_empty_not_an_error() {
        let mut wb = phonetic_workbook();
        assert_eq!(wb.cell_string_state(0, at("J1")).unwrap(), None);
    }

    #[test]
    fn a_snapshot_node_reads_like_the_live_cell_before_a_write_and_differs_after() {
        let (mut wb, _) = open();
        let snap = wb.snapshot_cell(0, at("A1")).unwrap();
        let node = snap.node().unwrap().clone();
        assert_eq!(wb.string_state_of(&node, at("A1")).unwrap(), wb.cell_string_state(0, at("A1")).unwrap());
        let snap3 = wb.snapshot_cell(0, at("B3")).unwrap();
        assert_eq!(FormulaState::of(snap3.node().unwrap()), wb.cell_formula_state(0, at("B3")).unwrap());
        assert!(FormulaState::of(snap3.node().unwrap()).is_some());

        wb.edit_cell(0, at("A1"), CellOp::Text("other".into())).unwrap();
        wb.edit_cell(0, at("B3"), CellOp::Number("1".into())).unwrap();
        assert_ne!(wb.string_state_of(&node, at("A1")).unwrap(), wb.cell_string_state(0, at("A1")).unwrap(), "the snapshot still has the rich runs");
        assert!(wb.string_state_of(&node, at("A1")).unwrap().unwrap().runs.is_some());
        assert_ne!(FormulaState::of(snap3.node().unwrap()), wb.cell_formula_state(0, at("B3")).unwrap());
    }

    #[test]
    fn a_rich_string_written_in_the_session_is_read_from_the_live_table() {
        let (mut wb, _) = open();
        let font = crate::Font { bold: true, ..crate::Font::default() };
        wb.worksheet().unwrap().cell_mut(5, 5).unwrap().set_rich_string(&[(&font, "ab"), (&crate::Font::default(), "cd")]).unwrap();
        let r = runs(&mut wb, "F6").expect("rich");
        assert_eq!(r.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["ab", "cd"]);
        assert!(r[0].props.as_deref().unwrap().contains("}b"));
        // and an old one still reads
        assert_eq!(runs(&mut wb, "A1").unwrap()[0].text, "Bo");
    }

    #[test]
    fn a_table_behind_alternate_content_is_an_error() {
        let sst = sst_xml("").replace("<si>", r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x"><si><t>z</t></si></mc:Choice></mc:AlternateContent><si>"#);
        let (mut wb, _) = open_with(&sheet_xml(), &sst);
        assert!(wb.cell_string_state(0, at("A1")).is_err());
    }

    #[test]
    fn canonical_keeps_text_content() {
        let parse = |x: &str| ooxml_core::XmlDocument::parse(x.as_bytes()).unwrap();
        let (a, b, c) = (parse("<x>a</x>"), parse("<x>b</x>"), parse("<x>a</x>"));
        assert_ne!(canonical(a.root()), canonical(b.root()));
        assert_eq!(canonical(a.root()), canonical(c.root()));
        // text next to elements counts, whitespace-only text between elements does not
        let (d, e, f) = (parse("<x>t<y/></x>"), parse("<x><y/></x>"), parse("<x> <y/>\n</x>"));
        assert_ne!(canonical(d.root()), canonical(e.root()));
        assert_eq!(canonical(e.root()), canonical(f.root()));
    }
}
