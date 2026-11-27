//! Phase4 part 2 (ADR-0014 §8–§10): sheet views and panes, columns and rows,
//! page setup, margins, print options, header/footer and manual breaks.
//!
//! Every call checks first without changing anything (`Err` ⇒ every byte
//! unchanged), then edits in place under ADR-0003 §3.1's poisoning.

use ooxml_core::{ElementSpec, LensMut, QName, RawNode};
use ooxml_schema::sml;

use crate::error::{Error, Result};
use crate::reference::{CellIndex, MAX_COLS, MAX_ROWS};
use crate::workbook::Workbook;
use crate::worksheet::{Worksheet, refuse_alternate_content};

/// A worksheet row page break with zero-based inclusive column bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowPageBreak {
    pub id: u32,
    pub min: u32,
    pub max: u32,
    pub manual: bool,
}

enum OutlineProperties {}

impl ElementSpec for OutlineProperties {
    const QNAME: QName = QName::new(sml::NS_X, "outlinePr");
    const SDK_NAME: &'static str = "OutlineProperties";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Empty;
}

impl ooxml_core::ChildOf<sml::SheetProperties> for OutlineProperties {}

const fn q(local: &'static str) -> QName {
    QName::new("", local)
}

/// Sets attribute `name` to `v`, or removes it for `None`.
fn put<E: ElementSpec>(e: &mut LensMut<'_, E>, name: &'static str, v: Option<&str>) -> Result<()> {
    match v {
        Some(v) => e.set_attr(q(name), v)?,
        None => {
            e.remove_attr(q(name))?;
        }
    }
    Ok(())
}

fn setting(what: &str, reason: impl Into<String>) -> Error {
    Error::InvalidSetting { what: what.into(), reason: reason.into() }
}

fn num(x: f64) -> String {
    // the shortest round-tripping form, without a trailing ".0"
    let s = format!("{x}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_owned)
}

const PRINTER_SETTINGS: &[&str] = &[
    "pageOrder", "cellComments", "errors", "blackAndWhite", "draft",
    "useFirstPageNumber", "usePrinterDefaults", "horizontalDpi", "verticalDpi",
    "firstPageNumber", "copies",
];

/// Page orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Portrait.
    Portrait,
    /// Landscape.
    Landscape,
}

/// `pageSetup` attributes to set (others, and the printer-settings `r:id`,
/// are kept).
#[derive(Clone, Debug, Default)]
pub struct PageSetup {
    /// `orientation`.
    pub orientation: Option<Orientation>,
    /// `paperSize` (ECMA-376 §18.3.1.63's codes; 9 = A4, 1 = Letter).
    pub paper_size: Option<u32>,
    /// `scale`, 10–400 (%).
    pub scale: Option<u32>,
    /// `fitToWidth` (pages; 0 = automatic).
    pub fit_to_width: Option<u32>,
    /// `fitToHeight` (pages; 0 = automatic).
    pub fit_to_height: Option<u32>,
    /// `sheetPr/pageSetUpPr/@fitToPage`: `Some(true)` fits to the
    /// `fitTo*` pages, `Some(false)` uses `scale`.
    pub fit_to_page: Option<bool>,
}

/// `pageMargins`, in inches; all six are written.
#[derive(Clone, Copy, Debug)]
pub struct Margins {
    /// Left.
    pub left: f64,
    /// Right.
    pub right: f64,
    /// Top.
    pub top: f64,
    /// Bottom.
    pub bottom: f64,
    /// Header.
    pub header: f64,
    /// Footer.
    pub footer: f64,
}

impl Default for Margins {
    /// Excel's defaults (as rust_xlsxwriter and POI write them).
    fn default() -> Self {
        Margins { left: 0.7, right: 0.7, top: 0.75, bottom: 0.75, header: 0.3, footer: 0.3 }
    }
}

/// `printOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrintOptions {
    /// Print gridlines.
    pub gridlines: bool,
    /// Print row and column headings.
    pub headings: bool,
    /// Centre horizontally.
    pub horizontal_centered: bool,
    /// Centre vertically.
    pub vertical_centered: bool,
}

/// `headerFooter` texts, with Excel's `&` codes (`&L`, `&C`, `&R`, `&P`,
/// `&N`, `&D`, `&T`, `&A`, `&F`, `&Z`, `&B`, `&I`, `&U`, `&E`, `&S`, `&X`,
/// `&Y`, `&O`, `&H`, `&&`, `&"font,style"`, `&nn` size, `&Kcolor`). `None`
/// removes that text. Images (`&G`) are out of scope.
#[derive(Clone, Debug, Default)]
pub struct HeaderFooter {
    /// `oddHeader` (every page unless the others apply).
    pub odd_header: Option<String>,
    /// `oddFooter`.
    pub odd_footer: Option<String>,
    /// `evenHeader` (with `different_odd_even`).
    pub even_header: Option<String>,
    /// `evenFooter`.
    pub even_footer: Option<String>,
    /// `firstHeader` (with `different_first`).
    pub first_header: Option<String>,
    /// `firstFooter`.
    pub first_footer: Option<String>,
    /// `differentOddEven`.
    pub different_odd_even: bool,
    /// `differentFirst`.
    pub different_first: bool,
}

/// Validates a header/footer string: at most 255 UTF-16 units, codes
/// included (the stricter reading, ADR-0014 §10), and only known `&` codes.
pub(crate) fn check_header_footer(s: &str) -> Result<()> {
    let bad = |m: String| Err(Error::InvalidHeaderFooter(m));
    if s.encode_utf16().count() > 255 {
        return bad(format!("{} characters (at most 255, codes included)", s.encode_utf16().count()));
    }
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '&' {
            continue;
        }
        match it.next() {
            Some('&' | 'L' | 'C' | 'R' | 'P' | 'N' | 'D' | 'T' | 'A' | 'F' | 'Z' | 'B' | 'I' | 'U' | 'E' | 'S' | 'X' | 'Y' | 'O' | 'H') => {}
            Some('"') => {
                if !it.by_ref().any(|c| c == '"') {
                    return bad("unterminated &\"font\" code".into());
                }
            }
            Some(d) if d.is_ascii_digit() => {
                while it.peek().is_some_and(char::is_ascii_digit) {
                    it.next();
                }
            }
            Some('K') => {
                // `&Krrggbb` or a theme colour `&Ktt+nnn` / `&Ktt-nnn`
                let code: String = it.by_ref().take(6).collect();
                let rgb = code.len() == 6 && code.chars().all(|c| c.is_ascii_hexdigit());
                let theme = code.len() == 6
                    && code[..2].chars().all(|c| c.is_ascii_digit())
                    && matches!(&code[2..3], "+" | "-")
                    && code[3..].chars().all(|c| c.is_ascii_digit());
                if !(rgb || theme) {
                    return bad(format!("&K{code}: not a colour"));
                }
            }
            Some('G') => return bad("&G (an image) is not supported".into()),
            Some(o) => return bad(format!("unknown code &{o}")),
            None => return bad("a trailing &".into()),
        }
    }
    Ok(())
}

const BOOK_VIEW_ATTRIBUTES: [&str; 7] = [
    "xWindow", "yWindow", "windowWidth", "windowHeight", "firstSheet", "activeTab", "visibility",
];

const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

impl Workbook {
    /// Reads the workbook theme palette in SpreadsheetML theme index order.
    pub fn theme_colors(&mut self) -> Result<[Option<String>; 12]> {
        const SLOTS: [&str; 12] = [
            "lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4",
            "accent5", "accent6", "hlink", "folHlink",
        ];

        let mut colors = std::array::from_fn(|_| None);
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let Some(part) = rels.by_type(REL_THEME).find_map(|rel| rels.resolve(rel)) else {
            return Ok(colors);
        };
        let doc = self.pkg.xml_part(&part)?;
        fn child<'a>(node: &'a RawNode, name: &'static str) -> Option<&'a RawNode> {
            node.elements().find(|n| n.is(QName::new(DRAWING, name)))
        }
        let Some(palette) = child(doc.root(), "themeElements").and_then(|node| child(node, "clrScheme")) else {
            return Ok(colors);
        };
        for (index, name) in SLOTS.into_iter().enumerate() {
            let Some(slot) = child(palette, name) else { continue };
            let rgb = child(slot, "srgbClr").and_then(|n| n.attr(QName::new("", "val")))
                .or_else(|| child(slot, "sysClr").and_then(|n| n.attr(QName::new("", "lastClr"))));
            colors[index] = rgb.filter(|v| v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_uppercase);
        }
        Ok(colors)
    }

    /// Restores the default theme while preserving existing theme part names.
    pub fn reset_theme(&mut self) -> Result<()> {
        self.set_theme_xml(include_str!("../template/blank/xl/theme/theme1.xml"))
    }

    /// Replaces the workbook theme with a DrawingML theme document.
    pub fn set_theme_xml(&mut self, xml: &str) -> Result<()> {
        use ooxml_core::package::PartNameRequest;
        const THEME_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

        self.check_editable()?;
        let bytes = xml.as_bytes();
        let document = ooxml_core::XmlDocument::parse_with(bytes, &self.pkg.parse_options())?;
        if !document.root().is(QName::new(DRAWING, "theme"))
            || !document.root().elements().any(|node| node.is(QName::new(DRAWING, "themeElements")))
        {
            return Err(setting("workbook theme", "expected a DrawingML theme document"));
        }
        let source = self.workbook_part.clone();
        let relationships = self.pkg.relationships(&source)?;
        let mut replacements = Vec::new();
        let mut has_theme = false;
        for relationship in relationships.by_type(REL_THEME) {
            let target = relationships.resolve(relationship)
                .filter(|target| self.pkg.contains(target))
                .ok_or_else(|| setting("workbook theme", "expected an existing internal theme part"))?;
            has_theme = true;
            if !replacements.contains(&target) && self.pkg.read_part(&target)? != bytes {
                replacements.push(target);
            }
        }
        if !has_theme {
            self.pkg.add_related_part(Some(&source), REL_THEME,
                PartNameRequest::Numbered("xl/theme/theme{}.xml".into()), THEME_TYPE, bytes.to_vec())?;
            return Ok(());
        }
        self.poisoning(|wb| {
            for target in replacements {
                wb.pkg.set_part(&target, bytes)?;
            }
            Ok(())
        })
    }

    /// Reads workbook windows in workbookViewId order.
    pub fn workbook_views(&mut self) -> Result<Vec<[String; 7]>> {
        let doc = self.pkg.xml_part(&self.workbook_part)?;
        let workbook = doc.lens::<sml::Workbook>()?;
        let Some(views) = workbook.child::<sml::BookViews>() else { return Ok(Vec::new()) };
        Ok(views.raw().elements().filter(|node| node.is(sml::WorkbookViewElement::QNAME))
            .map(|node| BOOK_VIEW_ATTRIBUTES.map(|name| node.attr(q(name)).unwrap_or_default().to_owned()))
            .collect())
    }

    /// Replaces the workbook windows without changing other workbook settings.
    pub fn set_workbook_views(&mut self, settings: &[[String; 7]]) -> Result<()> {
        self.check_editable()?;
        let part = self.workbook_part.clone();
        let doc = self.pkg.xml_part(&part)?;
        let workbook = doc.lens::<sml::Workbook>()?;
        refuse_alternate_content(workbook.raw(), sml::BookViews::QNAME)?;
        if let Some(views) = workbook.child::<sml::BookViews>() {
            refuse_alternate_content(views.raw(), sml::WorkbookViewElement::QNAME)?;
        } else if settings.is_empty() {
            return Ok(());
        }
        self.poisoning(|wb| {
            let mut workbook = wb.pkg.xml_part_mut(&part)?.lens_mut::<sml::Workbook>()?;
            if settings.is_empty() {
                let index = workbook.node().indexed_elements()
                    .find(|(_, child)| child.is(sml::BookViews::QNAME)).map(|(index, _)| index);
                if let Some(index) = index {
                    workbook.remove_at(index);
                }
                return Ok(());
            }
            let mut views = workbook.get_or_insert::<sml::BookViews>()?;
            let indices: Vec<_> = views.node().indexed_elements()
                .filter(|(_, child)| child.is(sml::WorkbookViewElement::QNAME))
                .map(|(index, _)| index).collect();
            for index in indices.into_iter().rev() {
                views.remove_at(index);
            }
            for values in settings {
                let mut view = views.insert::<sml::WorkbookViewElement>()?;
                for (name, value) in BOOK_VIEW_ATTRIBUTES.into_iter().zip(values) {
                    if !value.is_empty() {
                        view.set_attr(q(name), value)?;
                    }
                }
            }
            Ok(())
        })
    }

    fn sheet_part_of(&self, sheet: usize) -> String {
        self.sheets[sheet].part.clone().expect("worksheet")
    }

    /// The sheet view for a workbook window, as (sheetViews present,
    /// item index of the view, its node) — read only.
    fn view_by_id(&mut self, sheet: usize, id: u32) -> Result<(bool, Option<usize>, Option<RawNode>)> {
        let part = self.sheet_part_of(sheet);
        let doc = self.pkg.xml_part(&part)?;
        let ws = doc.lens::<sml::Worksheet>()?;
        let Some(views) = ws.child::<sml::SheetViews>() else { return Ok((false, None, None)) };
        let found = views
            .raw()
            .indexed_elements()
            .find(|(_, e)| e.is(sml::SheetViewElement::QNAME) && e.attr(q("workbookViewId")).is_some_and(|v| v.trim().parse::<u32>() == Ok(id)))
            .map(|(i, e)| (i, e.clone()));
        Ok((true, found.as_ref().map(|f| f.0), found.map(|f| f.1)))
    }

    /// Edits the requested workbook window view (created, with `sheetViews`, in
    /// their slots when absent): `plan` checks it read-only, `apply` edits it.
    fn with_view(
        &mut self,
        sheet: usize,
        id: u32,
        plan: impl FnOnce(Option<&RawNode>) -> Result<()>,
        apply: impl FnOnce(&mut LensMut<'_, sml::SheetViewElement>) -> Result<()>,
    ) -> Result<()> {
        self.check_editable()?;
        let (has_views, item, node) = self.view_by_id(sheet, id)?;
        plan(node.as_ref())?;
        let part = self.sheet_part_of(sheet);
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match (has_views, item) {
                (false, _) => ws.check_insert::<sml::SheetViews>()?,
                (true, None) => ws.child_mut::<sml::SheetViews>().expect("present").check_insert::<sml::SheetViewElement>()?,
                (true, Some(_)) => {}
            }
        }
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut views = ws.get_or_insert::<sml::SheetViews>()?;
            let mut view = match item {
                Some(i) => views.into_child_at::<sml::SheetViewElement>(i).expect("planned"),
                None => {
                    let mut v = views.insert::<sml::SheetViewElement>()?;
                    v.set_attr(q("workbookViewId"), &id.to_string())?;
                    let i = *v.path().last().expect("child");
                    views.into_child_at::<sml::SheetViewElement>(i).expect("inserted")
                }
            };
            apply(&mut view)
        })
    }
}

/// The `activeCell`/`sqref` to carry over: the selection of the old active
/// pane, else the first one.
fn carried_selection(view: Option<&RawNode>) -> (Option<String>, Option<String>) {
    let Some(v) = view else { return (None, None) };
    let active = v.elements().find(|e| e.is(sml::Pane::QNAME)).and_then(|p| p.attr(q("activePane")).map(str::to_owned));
    let sels: Vec<&RawNode> = v.elements().filter(|e| e.is(sml::Selection::QNAME)).collect();
    let pick = sels.iter().find(|s| s.attr(q("pane")).map(str::to_owned) == active).or(sels.first());
    pick.map_or((None, None), |s| (s.attr(q("activeCell")).map(str::to_owned), s.attr(q("sqref")).map(str::to_owned)))
}

/// Removes the view's `pane` and every `selection`.
fn clear_panes(view: &mut LensMut<'_, sml::SheetViewElement>) {
    let doomed: Vec<usize> = view
        .node()
        .indexed_elements()
        .filter(|(_, e)| e.is(sml::Pane::QNAME) || e.is(sml::Selection::QNAME))
        .map(|(i, _)| i)
        .collect();
    for i in doomed.into_iter().rev() {
        view.remove_at(i);
    }
}

fn add_selection(view: &mut LensMut<'_, sml::SheetViewElement>, pane: Option<&str>, cell: &(Option<String>, Option<String>)) -> Result<()> {
    let mut s = view.insert::<sml::Selection>()?;
    if let Some(p) = pane {
        s.set_attr(q("pane"), p)?;
    }
    if let Some(a) = &cell.0 {
        s.set_attr(q("activeCell"), a)?;
    }
    if let Some(r) = &cell.1 {
        s.set_attr(q("sqref"), r)?;
    }
    Ok(())
}

/// Which pane is active for these splits.
fn active_pane(x: bool, y: bool) -> &'static str {
    match (x, y) {
        (true, true) => "bottomRight",
        (false, true) => "bottomLeft",
        _ => "topRight",
    }
}

impl Worksheet<'_> {
    /// Reads row and column layout records, each as kind, start, end, size, hidden and outline.
    pub fn layout_records(&mut self) -> Result<Vec<String>> {
        let part = self.wb.sheet_part_of(self.sheet);
        let doc = self.wb.pkg.xml_part(&part)?;
        let sheet = doc.lens::<sml::Worksheet>()?;
        let mut records = Vec::new();
        let mut add = |kind: &str, node: &RawNode, keys: [&'static str; 5]| {
            records.push(kind.to_owned());
            records.extend(keys.map(|key| node.attr(q(key)).unwrap_or_default().to_owned()));
        };
        for columns in sheet.children::<sml::Columns>() {
            for column in columns.raw().elements().filter(|node| node.is(sml::Column::QNAME)) {
                add("column", column, ["min", "max", "width", "hidden", "outlineLevel"]);
            }
        }
        if let Some(data) = sheet.child::<sml::SheetData>() {
            for row in data.raw().elements().filter(|node| node.is(sml::Row::QNAME)) {
                if ["ht", "hidden", "outlineLevel"].iter().any(|key| row.attr(q(key)).is_some()) {
                    add("row", row, ["r", "r", "ht", "hidden", "outlineLevel"]);
                }
            }
        }
        Ok(records)
    }

    /// Selects the workbook window targeted by view edits on this handle.
    pub fn for_workbook_view(mut self, id: u32) -> Self {
        self.workbook_view_id = id;
        self
    }

    /// Replaces the sheet views with empty views for the given window IDs.
    pub fn reset_sheet_views(&mut self, ids: &[u32]) -> Result<()> {
        self.wb.check_editable()?;
        let part = self.wb.sheet_part_of(self.sheet);
        let index = {
            let doc = self.wb.pkg.xml_part(&part)?;
            let sheet = doc.lens::<sml::Worksheet>()?;
            refuse_alternate_content(sheet.raw(), sml::SheetViews::QNAME)?;
            sheet.raw().indexed_elements().find(|(_, child)| child.is(sml::SheetViews::QNAME))
                .map(|(index, _)| index)
        };
        if ids.is_empty() && index.is_none() {
            return Ok(());
        }
        self.wb.poisoning(|wb| {
            let mut sheet = wb.pkg.xml_part_mut(&part)?.lens_mut::<sml::Worksheet>()?;
            if let Some(index) = index {
                sheet.remove_at(index);
            }
            if !ids.is_empty() {
                let mut views = sheet.insert::<sml::SheetViews>()?;
                for id in ids {
                    views.insert::<sml::SheetViewElement>()?.set_attr(q("workbookViewId"), &id.to_string())?;
                }
            }
            Ok(())
        })
    }

    /// Reads the primary worksheet view settings used by document adapters.
    pub fn primary_view(&mut self) -> Result<Option<Vec<(String, String)>>> {
        let (_, _, node) = self.wb.view_by_id(self.sheet, 0)?;
        Ok(node.as_ref().map(Self::read_view))
    }

    /// Reads all worksheet views in document order.
    pub fn sheet_views(&mut self) -> Result<Vec<Vec<(String, String)>>> {
        let part = self.wb.sheet_part_of(self.sheet);
        let doc = self.wb.pkg.xml_part(&part)?;
        let sheet = doc.lens::<sml::Worksheet>()?;
        let Some(views) = sheet.child::<sml::SheetViews>() else { return Ok(Vec::new()) };
        Ok(views.raw().elements()
            .filter(|node| node.is(sml::SheetViewElement::QNAME))
            .map(Self::read_view)
            .collect())
    }

    fn read_view(view: &RawNode) -> Vec<(String, String)> {
        let mut settings = Vec::new();
        for name in ["workbookViewId", "zoomScale", "zoomScaleNormal", "showGridLines", "rightToLeft", "showRuler", "showRowColHeaders", "tabSelected", "view"] {
            if let Some(value) = view.attr(q(name)) {
                settings.push((name.to_owned(), value.to_owned()));
            }
        }
        let pane = view.elements().find(|node| node.is(sml::Pane::QNAME));
        if let Some(pane) = pane {
            for name in ["state", "xSplit", "ySplit", "topLeftCell", "activePane"] {
                if let Some(value) = pane.attr(q(name)) {
                    settings.push((name.to_owned(), value.to_owned()));
                }
            }
            if pane.attr(q("state")).is_none() {
                settings.push(("state".into(), "split".into()));
            }
        } else {
            settings.push(("state".into(), "normal".into()));
        }
        let active_pane = pane.and_then(|node| node.attr(q("activePane"))).unwrap_or("topLeft");
        let active_cell = view.elements()
            .filter(|node| node.is(sml::Selection::QNAME))
            .find(|node| node.attr(q("pane")).unwrap_or("topLeft") == active_pane)
            .and_then(|node| node.attr(q("activeCell")));
        if let Some(active) = active_cell {
            settings.push(("activeCell".into(), active.to_owned()));
        }
        settings
    }

    /// Updates the pane's top-left cell and the active selection of the primary view.
    pub fn set_view_cells(&mut self, top_left: Option<&str>, active_cell: Option<&str>) -> Result<()> {
        self.set_view_selection(top_left, active_cell, None)
    }

    /// Updates the cells and optionally selects a specific pane of a split view.
    pub fn set_view_selection(
        &mut self,
        top_left: Option<&str>,
        active_cell: Option<&str>,
        active_pane: Option<&str>,
    ) -> Result<()> {
        if active_pane.is_some_and(|pane| !matches!(pane, "topLeft" | "topRight" | "bottomLeft" | "bottomRight")) {
            return Err(setting("active pane", "unknown pane"));
        }
        let top_left = top_left.map(CellIndex::parse).transpose()?.map(|cell| cell.to_string());
        let active_cell = active_cell.map(CellIndex::parse).transpose()?.map(|cell| cell.to_string());
        self.wb.with_view(
            self.sheet,
            self.workbook_view_id,
            |view| {
                let pane = view.and_then(|node| node.elements().find(|child| child.is(sml::Pane::QNAME)));
                if active_pane.is_some() && !pane.is_some_and(|node| matches!(node.attr(q("state")), None | Some("split"))) {
                    return Err(setting("active pane", "requires a split view"));
                }
                if top_left.is_some() && pane.is_none() {
                    return Err(setting("view top-left cell", "requires a pane"));
                }
                Ok(())
            },
            |view| {
                let active_pane = if let Some(mut pane) = view.child_mut::<sml::Pane>() {
                    if let Some(cell) = &top_left {
                        pane.set_attr(q("topLeftCell"), cell)?;
                    }
                    if let Some(active) = active_pane {
                        pane.set_attr(q("activePane"), active)?;
                    }
                    pane.node().attr(q("activePane")).map(str::to_owned)
                } else {
                    None
                };
                if let Some(cell) = &active_cell {
                    let pane = active_pane.as_deref();
                    if let Some(mut selection) = view.child_mut_by::<sml::Selection>(|node| node.attr(q("pane")) == pane) {
                        selection.set_attr(q("activeCell"), cell)?;
                        selection.set_attr(q("sqref"), cell)?;
                    } else {
                        add_selection(view, pane, &(Some(cell.clone()), Some(cell.clone())))?;
                    }
                }
                Ok(())
            },
        )
    }

    /// Freezes the first `rows` rows and `cols` columns (ADR-0014 §8) in
    /// the `workbookViewId="0"` view: a `pane` with `state="frozen"`, and
    /// the `selection`s replaced by one per pane, the active cell carried
    /// over. Errors: [`Error::NothingToFreeze`], [`Error::PageLayoutView`].
    pub fn freeze_panes(&mut self, rows: u32, cols: u32) -> Result<()> {
        if rows == 0 && cols == 0 {
            return Err(Error::NothingToFreeze);
        }
        if rows >= MAX_ROWS || cols >= MAX_COLS {
            return Err(setting("freeze_panes", "beyond the sheet"));
        }
        let (x, y) = (cols > 0, rows > 0);
        let top_left = CellIndex { row: rows, col: cols }.to_string();
        let sheet = self.sheet;
        let carried_cell = std::cell::RefCell::new((None, None));
        self.wb.with_view(
            sheet,
            self.workbook_view_id,
            |v| {
                if v.is_some_and(|v| v.attr(q("view")) == Some("pageLayout")) {
                    return Err(Error::PageLayoutView);
                }
                *carried_cell.borrow_mut() = carried_selection(v);
                Ok(())
            },
            |view| {
                let cell = carried_cell.borrow().clone();
                clear_panes(view);
                let mut pane = view.insert::<sml::Pane>()?;
                if x {
                    pane.set_attr(q("xSplit"), &cols.to_string())?;
                }
                if y {
                    pane.set_attr(q("ySplit"), &rows.to_string())?;
                }
                pane.set_attr(q("topLeftCell"), &top_left)?;
                pane.set_attr(q("activePane"), active_pane(x, y))?;
                pane.set_attr(q("state"), "frozen")?;
                if x && y {
                    add_selection(view, Some("topRight"), &(None, None))?;
                    add_selection(view, Some("bottomLeft"), &(None, None))?;
                }
                add_selection(view, Some(active_pane(x, y)), &cell)
            },
        )
    }

    /// Splits the view at `x_twips`/`y_twips` (twentieths of a point) with
    /// `state="split"` (ADR-0014 §8).
    pub fn split_panes(&mut self, x_twips: u32, y_twips: u32) -> Result<()> {
        self.split_panes_at(f64::from(x_twips), f64::from(y_twips))
    }

    /// Splits the view at offsets expressed in fractional twips.
    pub fn split_panes_at(&mut self, x_twips: f64, y_twips: f64) -> Result<()> {
        if [x_twips, y_twips].into_iter().any(|value| !value.is_finite() || value < 0.0) {
            return Err(setting("split panes", "offsets must be finite and nonnegative"));
        }
        if x_twips == 0.0 && y_twips == 0.0 {
            return Err(Error::NothingToFreeze);
        }
        let sheet = self.sheet;
        let carried_cell = std::cell::RefCell::new((None, None));
        let (x, y) = (x_twips > 0.0, y_twips > 0.0);
        self.wb.with_view(
            sheet,
            self.workbook_view_id,
            |v| {
                if v.is_some_and(|v| v.attr(q("view")) == Some("pageLayout")) {
                    return Err(Error::PageLayoutView);
                }
                *carried_cell.borrow_mut() = carried_selection(v);
                Ok(())
            },
            |view| {
                let cell = carried_cell.borrow().clone();
                clear_panes(view);
                let mut pane = view.insert::<sml::Pane>()?;
                if x {
                    pane.set_attr(q("xSplit"), &x_twips.to_string())?;
                }
                if y {
                    pane.set_attr(q("ySplit"), &y_twips.to_string())?;
                }
                pane.set_attr(q("activePane"), active_pane(x, y))?;
                pane.set_attr(q("state"), "split")?;
                add_selection(view, Some(active_pane(x, y)), &cell)
            },
        )
    }

    /// Removes the view's pane and its pane-specific `selection`s, keeping
    /// one pane-less `selection` with the active cell; `Ok(false)` if there
    /// is no pane.
    pub fn unfreeze(&mut self) -> Result<bool> {
        let sheet = self.sheet;
        let (_, _, node) = self.wb.view_by_id(sheet, self.workbook_view_id)?;
        if !node.as_ref().is_some_and(|v| v.elements().any(|e| e.is(sml::Pane::QNAME))) {
            self.wb.check_editable()?;
            return Ok(false);
        }
        let cell = carried_selection(node.as_ref());
        self.wb.with_view(
            sheet,
            self.workbook_view_id,
            |_| Ok(()),
            |view| {
                clear_panes(view);
                add_selection(view, None, &cell)
            },
        )?;
        Ok(true)
    }

    /// `zoomScale` of the view, 10–400 (%).
    pub fn set_zoom(&mut self, percent: u32) -> Result<()> {
        if !(10..=400).contains(&percent) {
            return Err(setting("zoom", format!("{percent} is outside 10–400")));
        }
        self.wb.with_view(self.sheet, self.workbook_view_id, |_| Ok(()), |v| Ok(v.set_attr(q("zoomScale"), &percent.to_string())?))
    }

    /// Updates the optional display settings of the primary view.
    pub fn set_view_display(
        &mut self,
        normal_zoom: Option<u32>,
        ruler: Option<bool>,
        headings: Option<bool>,
        selected: Option<bool>,
        style: Option<&str>,
    ) -> Result<()> {
        if let Some(zoom) = normal_zoom && !(10..=400).contains(&zoom) {
            return Err(setting("normal zoom", format!("{zoom} is outside 10–400")));
        }
        if let Some(style) = style && !matches!(style, "normal" | "pageBreakPreview" | "pageLayout") {
            return Err(setting("view style", format!("unknown view style {style}")));
        }
        self.wb.with_view(self.sheet, self.workbook_view_id, |_| Ok(()), |view| {
            if let Some(zoom) = normal_zoom {
                put(view, "zoomScaleNormal", Some(&zoom.to_string()))?;
            }
            for (name, value, default) in [
                ("showRuler", ruler, true),
                ("showRowColHeaders", headings, true),
                ("tabSelected", selected, false),
            ] {
                if let Some(value) = value {
                    put(view, name, (value != default).then_some(if value { "1" } else { "0" }))?;
                }
            }
            if let Some(style) = style {
                put(view, "view", Some(style))?;
            }
            Ok(())
        })
    }

    /// `showGridLines` (default shown; `false` writes `0`).
    pub fn set_show_gridlines(&mut self, show: bool) -> Result<()> {
        self.wb.with_view(self.sheet, self.workbook_view_id, |_| Ok(()), |v| put(v, "showGridLines", (!show).then_some("0")))
    }

    /// `rightToLeft` (default off).
    pub fn set_right_to_left(&mut self, on: bool) -> Result<()> {
        self.wb.with_view(self.sheet, self.workbook_view_id, |_| Ok(()), |v| put(v, "rightToLeft", on.then_some("1")))
    }
}

// ---- columns and rows (§9) ----

/// What a column setter changes.
#[derive(Clone, Copy, Debug)]
enum ColEdit {
    Width(f64),
    ClearWidth,
    Style(u32),
    Hidden(bool),
    Outline(u8),
}

/// An existing `col`: item index, `min`, `max`, its attributes (static
/// names), and whether every attribute could be named statically.
type ExistingCol = (usize, u32, u32, Vec<(QName, String)>, bool);

/// One `col` to write: its range and attributes (static names).
#[derive(Clone, Debug)]
struct NewCol {
    min: u32,
    max: u32,
    attrs: Vec<(QName, String)>,
    /// A piece of a split col: its attributes are the original's, in the
    /// original order, and the setter's edit is applied after (as on an
    /// edited col), so the bytes stay as close to Excel's as they can.
    edit_after: bool,
}

fn col_attr_name(a: &ooxml_core::RawAttr) -> Option<QName> {
    sml::Column::ATTRS.iter().map(|s| s.name).find(|n| n.ns == a.name().ns() && n.local == a.name().local())
}

fn apply_edit(attrs: &mut Vec<(QName, String)>, e: ColEdit) {
    let mut set = |name: &'static str, v: Option<String>| {
        attrs.retain(|(n, _)| n.local != name);
        if let Some(v) = v {
            attrs.push((q(name), v));
        }
    };
    match e {
        ColEdit::Width(w) => {
            set("width", Some(num(w)));
            set("customWidth", Some("1".into()));
        }
        ColEdit::ClearWidth => {
            set("width", None);
            set("customWidth", None);
        }
        ColEdit::Style(index) => set("style", (index != 0).then(|| index.to_string())),
        ColEdit::Hidden(h) => set("hidden", h.then(|| "1".into())),
        ColEdit::Outline(l) => set("outlineLevel", (l > 0).then(|| l.to_string())),
    }
}

/// The schema order of `col`'s attributes, for new elements.
fn ordered(mut attrs: Vec<(QName, String)>) -> Vec<(QName, String)> {
    let rank = |n: &QName| sml::Column::ATTRS.iter().position(|s| s.name == *n).unwrap_or(usize::MAX);
    attrs.sort_by_key(|(n, _)| rank(n));
    attrs
}

impl Workbook {
    fn set_columns(&mut self, sheet: usize, first: u32, last: u32, edit: ColEdit) -> Result<()> {
        self.check_editable()?;
        if first > last || last >= MAX_COLS {
            return Err(setting("columns", format!("{first}..={last} is not a column range")));
        }
        match edit {
            ColEdit::Width(w) if !(0.0..=255.0).contains(&w) || w.is_nan() => return Err(setting("column width", format!("{w} is outside 0–255"))),
            ColEdit::Outline(l) if l > 7 => return Err(Error::InvalidOutlineLevel(l)),
            _ => {}
        }
        let (a, b) = (first + 1, last + 1); // `col/@min`/`@max` are 1-based
        let part = self.sheet_part_of(sheet);
        // ---- plan ----
        let doc = self.pkg.xml_part(&part)?;
        let ws = doc.lens::<sml::Worksheet>()?;
        let cols_count = ws.raw().elements().filter(|e| e.is(sml::Columns::QNAME)).count();
        if cols_count > 1 {
            return Err(Error::AmbiguousColumns("more than one cols element".into()));
        }
        let default_width = {
            let fmt = ws.child::<sml::SheetFormatProperties>();
            let attr = |n| fmt.as_ref().and_then(|f| f.raw().attr(q(n))).and_then(|v| v.trim().parse::<f64>().ok());
            attr("defaultColWidth").unwrap_or(attr("baseColWidth").unwrap_or(8.0) + 5.0)
        };
        let existing: Vec<ExistingCol> = match ws.child::<sml::Columns>() {
            None => Vec::new(),
            Some(c) => {
                let mut v = Vec::new();
                for (i, e) in c.raw().indexed_elements().filter(|(_, e)| e.is(sml::Column::QNAME)) {
                    let p = |n| e.attr(q(n)).and_then(|x| x.trim().parse::<u32>().ok());
                    let (Some(m), Some(n)) = (p("min"), p("max")) else {
                        return Err(Error::AmbiguousColumns("a col without min/max".into()));
                    };
                    let mut attrs = Vec::new();
                    let mut copyable = true;
                    for at in e.attrs() {
                        match col_attr_name(at) {
                            Some(name) => attrs.push((name, at.value().to_owned())),
                            None => copyable = false,
                        }
                    }
                    v.push((i, m, n, attrs, copyable));
                }
                v
            }
        };
        let mut sorted: Vec<(u32, u32)> = existing.iter().map(|e| (e.1, e.2)).collect();
        sorted.sort_unstable();
        if sorted.windows(2).any(|w| w[1].0 <= w[0].1) || existing.windows(2).any(|w| w[1].1 < w[0].1) {
            return Err(Error::AmbiguousColumns("overlapping or unordered col ranges".into()));
        }
        // the operations, by item index (descending when applied)
        enum Op {
            Edit(usize),
            Replace(usize, Vec<NewCol>),
            InsertBefore(usize, NewCol),
            Append(NewCol),
        }
        let mut ops = Vec::new();
        let mut cursor = a; // first column of [a, b] not yet covered
        for (i, m, n, attrs, copyable) in &existing {
            let (i, m, n) = (*i, *m, *n);
            if n < a || m > b {
                continue;
            }
            if cursor < m && !matches!(edit, ColEdit::ClearWidth | ColEdit::Style(0)) {
                // a gap before this col
                let mut at = vec![(q("min"), cursor.to_string()), (q("max"), (m - 1).to_string()), (q("width"), num(default_width))];
                apply_edit(&mut at, edit);
                ops.push(Op::InsertBefore(i, NewCol { min: cursor, max: m - 1, attrs: ordered(at), edit_after: false }));
            }
            if m >= a && n <= b {
                ops.push(Op::Edit(i));
            } else {
                if !copyable {
                    return Err(Error::AmbiguousColumns(format!("col {m}-{n} has an attribute this crate cannot copy when splitting")));
                }
                let piece = |lo: u32, hi: u32, edited: bool| {
                    let at: Vec<(QName, String)> = attrs
                        .iter()
                        .map(|(k, v)| match k.local {
                            "min" => (*k, lo.to_string()),
                            "max" => (*k, hi.to_string()),
                            _ => (*k, v.clone()),
                        })
                        .collect();
                    NewCol { min: lo, max: hi, attrs: at, edit_after: edited }
                };
                let mut parts = Vec::new();
                if m < a {
                    parts.push(piece(m, a - 1, false));
                }
                parts.push(piece(m.max(a), n.min(b), true));
                if n > b {
                    parts.push(piece(b + 1, n, false));
                }
                ops.push(Op::Replace(i, parts));
            }
            cursor = n + 1;
        }
        if cursor <= b && !matches!(edit, ColEdit::ClearWidth | ColEdit::Style(0)) {
            let next = existing.iter().find(|e| e.1 > b).map(|e| e.0);
            let mut at = vec![(q("min"), cursor.to_string()), (q("max"), b.to_string()), (q("width"), num(default_width))];
            apply_edit(&mut at, edit);
            let col = NewCol { min: cursor, max: b, attrs: ordered(at), edit_after: false };
            ops.push(match next {
                Some(i) => Op::InsertBefore(i, col),
                None => Op::Append(col),
            });
        }
        if ops.is_empty() {
            return Ok(());
        }
        // A split or insertion depends on the positions of every column.
        if let Some(cols) = ws.child::<sml::Columns>()
            && ops.iter().any(|op| !matches!(op, Op::Edit(_)))
        {
            refuse_alternate_content(cols.raw(), sml::Columns::QNAME)?;
        }
        let outline = match edit {
            ColEdit::Outline(l) => Some(l),
            _ => None,
        };
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if ws.child_mut::<sml::Columns>().is_none() {
                ws.check_insert::<sml::Columns>()?;
            }
            if outline.is_some_and(|l| l > 0) && ws.child_mut::<sml::SheetFormatProperties>().is_none() {
                ws.check_insert::<sml::SheetFormatProperties>()?;
            }
        }
        // ---- apply ----
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if let Some(l) = outline.filter(|&l| l > 0) {
                raise_outline(&mut ws, "outlineLevelCol", l)?;
            }
            let mut cols = ws.get_or_insert::<sml::Columns>()?;
            let new_col = |cols: &mut LensMut<'_, sml::Columns>, at: usize, c: &NewCol| -> Result<()> {
                let mut e = cols.insert_at::<sml::Column>(at)?;
                debug_assert!(c.min <= c.max);
                for (k, v) in &c.attrs {
                    e.set_attr(*k, v)?;
                }
                if c.edit_after {
                    edit_col(&mut e, edit)?;
                }
                Ok(())
            };
            // descending item order keeps the earlier indices valid
            ops.sort_by_key(|op| std::cmp::Reverse(match op {
                Op::Append(_) => (usize::MAX, 0),
                Op::Edit(i) | Op::Replace(i, _) => (*i, 1),
                Op::InsertBefore(i, _) => (*i, 0),
            }));
            for op in &ops {
                match op {
                    Op::Append(c) => {
                        let at = cols.node().children().len();
                        new_col(&mut cols, at, c)?;
                    }
                    Op::Edit(i) => edit_col(&mut cols.child_mut_at(*i)?, edit)?,
                    Op::Replace(i, parts) => {
                        cols.remove_at(*i);
                        for (k, c) in parts.iter().enumerate() {
                            new_col(&mut cols, i + k, c)?;
                        }
                    }
                    Op::InsertBefore(i, c) => new_col(&mut cols, *i, c)?,
                }
            }
            Ok(())
        })
    }

    fn set_row(&mut self, sheet: usize, row: u32, edit: RowEdit) -> Result<()> {
        self.check_editable()?;
        if row >= MAX_ROWS {
            return Err(setting("row", format!("{row} is beyond the sheet")));
        }
        match edit {
            RowEdit::Height(h) if !(0.0..=409.0).contains(&h) || h.is_nan() => return Err(setting("row height", format!("{h} is outside 0–409"))),
            RowEdit::Outline(l) if l > 7 => return Err(Error::InvalidOutlineLevel(l)),
            _ => {}
        }
        let part = self.sheet_part_of(sheet);
        self.ensure_index(sheet)?;
        let at = CellIndex { row, col: 0 };
        let loc = crate::worksheet::locate(self.pkg.xml_part(&part)?, self.indexes.get(&sheet).expect("built"), &part, at)?;
        if loc.row.is_none() && matches!(edit, RowEdit::ClearStyle | RowEdit::ClearHeight) {
            return Ok(());
        }
        let has_sd = self.indexes.get(&sheet).expect("built").sheet_data;
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if !has_sd {
                ws.check_insert::<sml::SheetData>()?;
            } else if loc.row.is_none() {
                ws.child_mut::<sml::SheetData>().expect("indexed").check_insert_at::<sml::Row>(loc.row_insert_at)?;
            }
            if matches!(edit, RowEdit::Outline(l) if l > 0) && ws.child_mut::<sml::SheetFormatProperties>().is_none() {
                ws.check_insert::<sml::SheetFormatProperties>()?;
            }
        }
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if let RowEdit::Outline(l) = edit
                && l > 0
            {
                raise_outline(&mut ws, "outlineLevelRow", l)?;
            }
            let created_sd = !has_sd;
            let mut sd = ws.get_or_insert::<sml::SheetData>()?;
            let (mut r, new_item) = match loc.row {
                Some(i) => (sd.into_child_at::<sml::Row>(i).expect("located"), None),
                None => {
                    let item = if created_sd { 0 } else { loc.row_insert_at };
                    let mut r = sd.insert_at::<sml::Row>(item)?;
                    r.set_attr(q("r"), &(row + 1).to_string())?;
                    let i = *r.path().last().expect("child");
                    (sd.into_child_at::<sml::Row>(i).expect("inserted"), Some(item))
                }
            };
            match edit {
                RowEdit::Height(h) => {
                    r.set_attr(q("ht"), &num(h))?;
                    r.set_attr(q("customHeight"), "1")?;
                }
                RowEdit::ClearHeight => {
                    r.remove_attr(q("ht"))?;
                    r.remove_attr(q("customHeight"))?;
                }
                RowEdit::Style(style) => {
                    r.set_attr(q("s"), &style.to_string())?;
                    r.set_attr(q("customFormat"), "1")?;
                }
                RowEdit::ClearStyle => {
                    r.remove_attr(q("s"))?;
                    r.remove_attr(q("customFormat"))?;
                }
                RowEdit::Hidden(h) => put(&mut r, "hidden", h.then_some("1"))?,
                RowEdit::Outline(l) => put(&mut r, "outlineLevel", (l > 0).then(|| l.to_string()).as_deref())?,
            }
            let index = wb.indexes.get_mut(&sheet).expect("built");
            if created_sd {
                index.sheet_data_created();
            }
            if let Some(item) = new_item {
                index.row_inserted(row + 1, item);
            }
            Ok(())
        })
    }
}

/// A column setter's change on an existing (or split-off) `col`.
fn edit_col(e: &mut LensMut<'_, sml::Column>, edit: ColEdit) -> Result<()> {
    let mut at: Vec<(QName, String)> = Vec::new();
    apply_edit(&mut at, edit);
    match edit {
        ColEdit::Width(_) => {
            e.set_attr(q("width"), &at[0].1)?;
            e.set_attr(q("customWidth"), "1")?;
        }
        ColEdit::ClearWidth => {
            e.remove_attr(q("width"))?;
            e.remove_attr(q("customWidth"))?;
        }
        ColEdit::Style(index) => put(e, "style", (index != 0).then(|| index.to_string()).as_deref())?,
        ColEdit::Hidden(h) => put(e, "hidden", h.then_some("1"))?,
        ColEdit::Outline(l) => put(e, "outlineLevel", (l > 0).then(|| l.to_string()).as_deref())?,
    }
    Ok(())
}

/// What a row setter changes.
#[derive(Clone, Copy, Debug)]
enum RowEdit {
    ClearHeight,
    Height(f64),
    Style(u32),
    ClearStyle,
    Hidden(bool),
    Outline(u8),
}

/// Raises `sheetFormatPr/@{attr}` to at least `level` (never lowers it);
/// `sheetFormatPr` is created with the required `defaultRowHeight`.
fn raise_outline(ws: &mut LensMut<'_, sml::Worksheet>, attr: &'static str, level: u8) -> Result<()> {
    let created = ws.child_mut::<sml::SheetFormatProperties>().is_none();
    let mut f = ws.get_or_insert::<sml::SheetFormatProperties>()?;
    if created {
        f.set_attr(q("defaultRowHeight"), "15")?;
    }
    let cur = f.node().attr(q(attr)).and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(0);
    if level > cur {
        f.set_attr(q(attr), &level.to_string())?;
    }
    Ok(())
}

trait ChildAt<'a> {
    fn child_mut_at(&mut self, i: usize) -> Result<LensMut<'_, sml::Column>>;
}

impl<'a> ChildAt<'a> for LensMut<'a, sml::Columns> {
    fn child_mut_at(&mut self, i: usize) -> Result<LensMut<'_, sml::Column>> {
        let k = self.node().indexed_elements().filter(|(_, e)| e.is(sml::Column::QNAME)).position(|(j, _)| j == i).expect("planned col");
        Ok(self.nth_child_mut::<sml::Column>(k).expect("planned col"))
    }
}

impl Worksheet<'_> {
    /// Returns the width, hidden flag, and outline level of a zero-based column.
    pub fn column_layout(&self, column: u32) -> Result<(Option<f64>, bool, u32)> {
        let (width, hidden, outline, _) = self.column_layout_and_style(column)?;
        Ok((width, hidden, outline))
    }

    /// Returns the default style index of a zero-based column.
    pub fn column_style_index(&self, column: u32) -> Result<u32> {
        Ok(self.column_layout_and_style(column)?.3)
    }

    /// Applies an existing cell format as the default style for the range.
    pub fn set_column_style_index(&mut self, first: u32, last: u32, index: u32) -> Result<()> {
        if index != 0 {
            let count = self.wb.cell_format_count()?;
            if index >= count {
                return Err(Error::StyleIndexOutOfRange { index, count });
            }
        }
        self.wb.set_columns(self.sheet, first, last, ColEdit::Style(index))
    }

    fn column_layout_and_style(&self, column: u32) -> Result<(Option<f64>, bool, u32, u32)> {
        let number = column.checked_add(1).filter(|&n| n <= 16384)
            .ok_or_else(|| setting("column", "outside XLSX bounds"))?;
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let ws = doc.lens::<sml::Worksheet>()?;
        let mut found = None;
        for cols in ws.raw().elements().filter(|e| e.is(sml::Columns::QNAME)) {
            for col in cols.elements().filter(|e| e.is(sml::Column::QNAME)) {
                let bound = |name| col.attr(q(name)).and_then(ooxml_core::value::parse_u32);
                let (Some(min), Some(max)) = (bound("min"), bound("max")) else {
                    return Err(Error::AmbiguousColumns("a col without min/max".into()));
                };
                if (min..=max).contains(&number) {
                    if found.is_some() {
                        return Err(Error::AmbiguousColumns("overlapping col ranges".into()));
                    }
                    let width = col.attr(q("width")).and_then(ooxml_core::value::parse_f64);
                    let hidden = col.attr(q("hidden")).and_then(ooxml_core::value::parse_bool).unwrap_or(false);
                    found = Some((width, hidden, bound("outlineLevel").unwrap_or(0), bound("style").unwrap_or(0)));
                }
            }
        }
        Ok(found.unwrap_or_default())
    }

    /// Sets the width (Excel character units, 0–255, `customWidth="1"`) of
    /// columns `first..=last` (0-based), splitting a `col` range that
    /// covers them only in part (ADR-0014 §9). Errors:
    /// [`Error::AmbiguousColumns`], [`Error::InvalidSetting`].
    pub fn set_column_width(&mut self, first: u32, last: u32, width: f64) -> Result<()> {
        self.wb.set_columns(self.sheet, first, last, ColEdit::Width(width))
    }

    /// Removes the explicit width of columns `first..=last`.
    pub fn clear_column_width(&mut self, first: u32, last: u32) -> Result<()> {
        self.wb.set_columns(self.sheet, first, last, ColEdit::ClearWidth)
    }

    /// Hides or shows columns `first..=last`.
    pub fn set_column_hidden(&mut self, first: u32, last: u32, hidden: bool) -> Result<()> {
        self.wb.set_columns(self.sheet, first, last, ColEdit::Hidden(hidden))
    }

    /// The outline level (0–7) of columns `first..=last`; raises
    /// `sheetFormatPr/@outlineLevelCol` when needed.
    pub fn set_column_outline(&mut self, first: u32, last: u32, level: u8) -> Result<()> {
        self.wb.set_columns(self.sheet, first, last, ColEdit::Outline(level))
    }

    /// Applies an existing cell format as the row's default.
    pub fn set_row_style_index(&mut self, row: u32, index: u32) -> Result<()> {
        let count = self.wb.cell_format_count()?;
        if index >= count {
            return Err(Error::StyleIndexOutOfRange { index, count });
        }
        self.wb.set_row(self.sheet, row, RowEdit::Style(index))
    }

    /// Removes a row's explicit default format.
    pub fn clear_row_style_index(&mut self, row: u32) -> Result<()> {
        self.wb.set_row(self.sheet, row, RowEdit::ClearStyle)
    }

    /// Sets row `row`'s height in points (0–409, `customHeight="1"`); an
    /// empty row is created in order when needed.
    pub fn set_row_height(&mut self, row: u32, height: f64) -> Result<()> {
        self.wb.set_row(self.sheet, row, RowEdit::Height(height))
    }

    /// Removes a row's explicit height while retaining its cells and other layout.
    pub fn clear_row_height(&mut self, row: u32) -> Result<()> {
        self.wb.set_row(self.sheet, row, RowEdit::ClearHeight)
    }

    /// Hides or shows row `row`.
    pub fn set_row_hidden(&mut self, row: u32, hidden: bool) -> Result<()> {
        self.wb.set_row(self.sheet, row, RowEdit::Hidden(hidden))
    }

    /// The outline level (0–7) of row `row`; raises
    /// `sheetFormatPr/@outlineLevelRow` when needed.
    pub fn set_row_outline(&mut self, row: u32, level: u8) -> Result<()> {
        self.wb.set_row(self.sheet, row, RowEdit::Outline(level))
    }
}

enum IgnoredErrors {}
impl ElementSpec for IgnoredErrors {
    const QNAME: QName = QName::new(crate::SML_NS, "ignoredErrors");
    const SDK_NAME: &'static str = "IgnoredErrors";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Slots(&[ooxml_core::Slot {
        names: &[IgnoredError::QNAME], wildcard: None, max_each: None, max_total: None,
    }]);
}
impl ooxml_core::ChildOf<sml::Worksheet> for IgnoredErrors {}

enum IgnoredError {}
impl ElementSpec for IgnoredError {
    const QNAME: QName = QName::new(crate::SML_NS, "ignoredError");
    const SDK_NAME: &'static str = "IgnoredError";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[
        ooxml_core::AttrSpec { name: QName::new("", "sqref"), ty: ooxml_core::SimpleType::String, default: None },
        ooxml_core::AttrSpec { name: QName::new("", "numberStoredAsText"), ty: ooxml_core::SimpleType::String, default: None },
    ];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Empty;
}
impl ooxml_core::ChildOf<IgnoredErrors> for IgnoredError {}

impl Worksheet<'_> {
    /// Toggles the number-stored-as-text warning for the declared worksheet range.
    /// Disabling leaves other ignored-error flags in place.
    pub fn set_ignore_numbers_stored_as_text(&mut self, enabled: bool) -> Result<()> {
        const FLAG: QName = QName::new("", "numberStoredAsText");
        const RANGE: QName = QName::new("", "sqref");
        self.wb.check_editable()?;
        let range = self.declared_range();
        if enabled && range.is_none() {
            return Ok(());
        }
        let part = self.wb.sheet_part_of(self.sheet);
        let mut doc = self.wb.pkg.xml_part(&part)?.clone();
        let mut sheet = doc.lens_mut::<sml::Worksheet>()?;
        let containers: Vec<_> = sheet.node().indexed_elements()
            .filter(|(_, node)| node.is(IgnoredErrors::QNAME)).map(|(index, _)| index).collect();
        if containers.len() > 1 {
            return Err(setting("ignored errors", "ambiguous ignoredErrors elements"));
        }
        if enabled {
            let range = range.expect("declared range checked");
            let mut errors = sheet.get_or_insert::<IgnoredErrors>()?;
            let covered = errors.node().elements().any(|node| {
                node.is(IgnoredError::QNAME) && matches!(node.attr(FLAG), Some("1" | "true"))
                    && node.attr(RANGE).into_iter().flat_map(str::split_ascii_whitespace)
                        .filter_map(|item| crate::CellRange::parse(item).ok())
                        .any(|item| item.contains(range.first) && item.contains(range.last))
            });
            if !covered {
                let mut error = errors.insert::<IgnoredError>()?;
                error.set_attr(RANGE, &range.to_string())?;
                error.set_attr(FLAG, "1")?;
            }
        } else if let Some(&index) = containers.first() {
            let errors = sheet.child_mut::<IgnoredErrors>().expect("known container");
            let candidates: Vec<_> = errors.node().indexed_elements()
                .filter(|(_, node)| node.is(IgnoredError::QNAME) && node.attr(FLAG).is_some())
                .map(|(index, node)| (index, node.attrs().iter().all(|attr| attr.name().ns().is_empty()
                    && matches!(attr.name().local(), "sqref" | "numberStoredAsText")) && node.children().is_empty()))
                .collect();
            for (position, empty) in candidates.into_iter().rev() {
                let mut errors = sheet.child_mut::<IgnoredErrors>().expect("known container");
                if empty {
                    errors.remove_at(position);
                } else {
                    errors.into_child_at::<IgnoredError>(position).expect("known error").remove_attr(FLAG)?;
                }
            }
            let errors = sheet.child_mut::<IgnoredErrors>().expect("known container");
            if errors.node().children().is_empty() && errors.node().attrs().is_empty() {
                sheet.remove_at(index);
            }
        }
        if doc.is_dirty() {
            let mut plan = ooxml_core::RegistryPlan::new(&self.wb.pkg);
            plan.stage_part(&self.wb.pkg, &part, doc)?;
            self.wb.apply_registry_edit(plan, |_| Ok(()))?;
        }
        Ok(())
    }
}

// ---- page setup (§10) ----

impl Workbook {
    /// Edits (or creates in its slot) the worksheet child `E`.
    fn edit_sheet_child<E>(&mut self, sheet: usize, apply: impl FnOnce(&mut LensMut<'_, E>) -> Result<()>) -> Result<()>
    where
        E: ooxml_core::ChildOf<sml::Worksheet>,
    {
        self.check_editable()?;
        let part = self.sheet_part_of(sheet);
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if ws.child_mut::<E>().is_none() {
                ws.check_insert::<E>()?;
            }
        }
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut e = ws.get_or_insert::<E>()?;
            apply(&mut e)
        })
    }
}

impl Worksheet<'_> {
    /// Reads the explicit summary direction flags for grouped rows and columns.
    pub fn outline_summary(&self) -> Result<(Option<bool>, Option<bool>)> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let sheet = doc.lens::<sml::Worksheet>()?;
        let outline = sheet.child::<sml::SheetProperties>()
            .and_then(|properties| properties.child::<OutlineProperties>());
        let flag = |name| outline.as_ref().and_then(|node| node.raw().attr(q(name)))
            .and_then(ooxml_core::value::parse_bool);
        Ok((flag("summaryBelow"), flag("summaryRight")))
    }

    /// Updates the supplied summary direction flags and preserves other outline preferences.
    pub fn set_outline_summary(&mut self, below: Option<bool>, right: Option<bool>) -> Result<()> {
        if below.is_none() && right.is_none() {
            return Ok(());
        }
        self.wb.edit_sheet_child::<sml::SheetProperties>(self.sheet, |properties| {
            let mut outline = properties.get_or_insert::<OutlineProperties>()?;
            for (name, value) in [("summaryBelow", below), ("summaryRight", right)] {
                if let Some(value) = value {
                    outline.set_attr(q(name), if value { "1" } else { "0" })?;
                }
            }
            Ok(())
        })
    }

    /// Reads the optional x14ac default row descent.
    pub fn default_row_descent(&self) -> Result<Option<f64>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let sheet = doc.lens::<sml::Worksheet>()?;
        let Some(format) = sheet.child::<sml::SheetFormatProperties>() else {
            return Ok(None);
        };
        Ok(sml::SheetFormatPropertiesView(format).dy_descent().map_err(ooxml_core::Error::from)?)
    }

    /// Sets the worksheet's x14ac default row descent.
    pub fn set_default_row_descent(&mut self, value: f64) -> Result<()> {
        if !value.is_finite() || value < 0.0 {
            return Err(setting("default row descent", "expected a finite nonnegative number"));
        }
        const DESCENT: QName = sml::SheetFormatProperties::ATTR_X14AC_DY_DESCENT;
        const IGNORABLE: QName = QName::new(ooxml_core::name::MC_NS, "Ignorable");
        self.wb.edit_sheet_child::<sml::SheetFormatProperties>(self.sheet, |format| {
            if format.node().attr(q("defaultRowHeight")).is_none() {
                format.set_attr(q("defaultRowHeight"), "15")?;
            }
            let prefix = format.ensure_namespace_prefix(ooxml_core::NsDecl::new("x14ac", DESCENT.ns))?;
            format.ensure_namespace_prefix(ooxml_core::NsDecl::new("mc", IGNORABLE.ns))?;
            let current = format.node().attr(IGNORABLE).unwrap_or_default();
            if !current.split_ascii_whitespace().any(|entry| entry == prefix) {
                let updated = if current.is_empty() { prefix } else { format!("{current} {prefix}") };
                format.set_attr(IGNORABLE, &updated)?;
            }
            format.set_attr(DESCENT, &num(value))?;
            Ok(())
        })
    }

    /// Reads the default row height and optional default column width.
    pub fn default_dimensions(&self) -> Result<(f64, Option<f64>)> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let sheet = doc.lens::<sml::Worksheet>()?;
        let format = sheet.child::<sml::SheetFormatProperties>();
        let dimension = |name| format.as_ref().and_then(|node| node.raw().attr(q(name)))
            .and_then(ooxml_core::value::parse_f64);
        Ok((dimension("defaultRowHeight").unwrap_or(15.0), dimension("defaultColWidth")))
    }

    /// Sets a worksheet's default row height or column width.
    pub fn set_default_dimension(&mut self, rows: bool, value: Option<f64>) -> Result<()> {
        let maximum = if rows { 409.0 } else { 255.0 };
        if value.is_some_and(|value| !(0.0..=maximum).contains(&value)) {
            return Err(setting("default dimension", format!("outside 0–{maximum}")));
        }
        self.wb.edit_sheet_child::<sml::SheetFormatProperties>(self.sheet, |format| {
            if rows || format.node().attr(q("defaultRowHeight")).is_none() {
                let height = if rows { value.unwrap_or(15.0) } else { 15.0 };
                format.set_attr(q("defaultRowHeight"), &num(height))?;
                if rows {
                    put(format, "customHeight", (height != 15.0).then_some("1"))?;
                }
            }
            if !rows {
                let width = value.map(num);
                put(format, "defaultColWidth", width.as_deref())?;
            }
            Ok(())
        })
    }

    /// Returns the worksheet's row and column outline levels.
    pub fn outline_levels(&self) -> Result<(u8, u8)> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let sheet = doc.lens::<sml::Worksheet>()?;
        let format = sheet.child::<sml::SheetFormatProperties>();
        let level = |name| format.as_ref().and_then(|node| node.raw().attr(q(name)))
            .and_then(|value| value.parse().ok()).unwrap_or(0);
        Ok((level("outlineLevelRow"), level("outlineLevelCol")))
    }

    /// Sets the outline level recorded in the worksheet format properties.
    pub fn set_outline_level(&mut self, rows: bool, level: u8) -> Result<()> {
        if level > 7 {
            return Err(setting("outline level", "outside 0–7"));
        }
        let attr = if rows { "outlineLevelRow" } else { "outlineLevelCol" };
        self.wb.edit_sheet_child::<sml::SheetFormatProperties>(self.sheet, |format| {
            if format.node().attr(q("defaultRowHeight")).is_none() {
                format.set_attr(q("defaultRowHeight"), "15")?;
            }
            format.set_attr(q(attr), &level.to_string())?;
            Ok(())
        })
    }

    /// Reads the stored worksheet tab color attributes.
    pub fn tab_color(&self) -> Result<Vec<(String, String)>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let worksheet = doc.lens::<sml::Worksheet>()?;
        let Some(color) = worksheet.child::<sml::SheetProperties>()
            .and_then(|properties| properties.child::<sml::TabColor>()) else {
            return Ok(Vec::new());
        };
        Ok(["rgb", "theme", "tint", "indexed", "auto"].into_iter()
            .filter_map(|key| color.raw().attr(q(key)).map(|value| (key.to_owned(), value.to_owned())))
            .collect())
    }

    /// Reads page setup, including the fit-to-page flag in sheet properties.
    pub fn page_setup(&self) -> Result<PageSetup> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let view = doc.lens::<sml::Worksheet>()?;
        let node = view.child::<sml::PageSetup>();
        let attr = |name| node.as_ref().and_then(|node| node.raw().attr(q(name)));
        let number = |name| attr(name).and_then(|value| value.parse().ok());
        let fit_to_page = view.child::<sml::SheetProperties>()
            .and_then(|properties| properties.child::<sml::PageSetupProperties>())
            .and_then(|properties| properties.raw().attr(q("fitToPage")))
            .and_then(ooxml_core::value::parse_bool);
        Ok(PageSetup {
            orientation: match attr("orientation") {
                Some("landscape") => Some(Orientation::Landscape),
                Some("portrait") => Some(Orientation::Portrait),
                _ => None,
            },
            paper_size: number("paperSize"),
            scale: number("scale"),
            fit_to_width: number("fitToWidth"),
            fit_to_height: number("fitToHeight"),
            fit_to_page,
        })
    }

    /// Sets `pageSetup` attributes (ADR-0014 §10); others and the
    /// printer-settings `r:id` are kept. `fit_to_page` also sets
    /// `sheetPr/pageSetUpPr/@fitToPage` (both created when absent).
    pub fn set_page_setup(&mut self, setup: &PageSetup) -> Result<()> {
        if let Some(s) = setup.scale
            && !(10..=400).contains(&s)
        {
            return Err(setting("scale", format!("{s} is outside 10–400")));
        }
        let sheet = self.sheet;
        if let Some(fit) = setup.fit_to_page {
            let part = self.wb.sheet_part_of(sheet);
            let doc = self.wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match ws.child_mut::<sml::SheetProperties>() {
                Some(mut pr) => {
                    if pr.child_mut::<sml::PageSetupProperties>().is_none() && fit {
                        pr.check_insert::<sml::PageSetupProperties>()?;
                    }
                }
                None if fit => ws.check_insert::<sml::SheetProperties>()?,
                None => {}
            }
        }
        let s = setup.clone();
        self.wb.edit_sheet_child::<sml::PageSetup>(sheet, |ps| {
            if let Some(o) = s.orientation {
                ps.set_attr(q("orientation"), if o == Orientation::Landscape { "landscape" } else { "portrait" })?;
            }
            for (name, v) in [("paperSize", s.paper_size), ("scale", s.scale), ("fitToWidth", s.fit_to_width), ("fitToHeight", s.fit_to_height)] {
                if let Some(v) = v {
                    ps.set_attr(q(name), &v.to_string())?;
                }
            }
            Ok(())
        })?;
        if let Some(fit) = setup.fit_to_page {
            let part = self.wb.sheet_part_of(sheet);
            self.wb.poisoning(|wb| {
                let doc = wb.pkg.xml_part_mut(&part)?;
                let mut ws = doc.lens_mut::<sml::Worksheet>()?;
                if !fit && ws.child_mut::<sml::SheetProperties>().is_none() {
                    return Ok(());
                }
                let mut pr = ws.get_or_insert::<sml::SheetProperties>()?;
                if !fit && pr.child_mut::<sml::PageSetupProperties>().is_none() {
                    return Ok(());
                }
                let mut p = pr.get_or_insert::<sml::PageSetupProperties>()?;
                put(&mut p, "fitToPage", fit.then_some("1"))
            })?;
        }
        Ok(())
    }

    /// Reads additional printer settings without changing the worksheet.
    pub fn printer_settings(&self) -> Result<Vec<(String, String)>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let view = doc.lens::<sml::Worksheet>()?;
        let Some(node) = view.child::<sml::PageSetup>() else {
            return Ok(Vec::new());
        };
        Ok(PRINTER_SETTINGS.iter().filter_map(|&name| {
            node.raw().attr(q(name)).map(|value| (name.to_owned(), value.to_owned()))
        }).collect())
    }

    /// Updates supported printer settings while preserving other page setup attributes.
    pub fn set_printer_settings(&mut self, settings: &[(String, String)]) -> Result<()> {
        let mut attributes = Vec::with_capacity(settings.len());
        for (name, value) in settings {
            let Some(&attribute) = PRINTER_SETTINGS.iter().find(|&&key| key == name) else {
                return Err(setting("printer setting", name.to_owned()));
            };
            let valid = match name.as_str() {
                "pageOrder" => matches!(value.as_str(), "downThenOver" | "overThenDown"),
                "cellComments" => matches!(value.as_str(), "none" | "asDisplayed" | "atEnd"),
                "errors" => matches!(value.as_str(), "displayed" | "blank" | "dash" | "NA"),
                "blackAndWhite" | "draft" | "useFirstPageNumber" | "usePrinterDefaults" => {
                    matches!(value.as_str(), "0" | "1")
                }
                "horizontalDpi" | "verticalDpi" | "firstPageNumber" | "copies" => value.parse::<u32>().is_ok(),
                _ => false,
            };
            if !valid {
                return Err(setting("printer setting", format!("{name} = {value}")));
            }
            attributes.push((attribute, value));
        }
        if attributes.is_empty() {
            return Ok(());
        }
        self.wb.edit_sheet_child::<sml::PageSetup>(self.sheet, |node| {
            for (name, value) in attributes {
                node.set_attr(q(name), value)?;
            }
            Ok(())
        })
    }

    /// Reports whether explicit page margins are present.
    pub fn has_page_margins(&self) -> Result<bool> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        Ok(doc.lens::<sml::Worksheet>()?.child::<sml::PageMargins>().is_some())
    }

    /// Reads the worksheet margins, using Excel defaults for missing values.
    pub fn margins(&self) -> Result<Margins> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let view = doc.lens::<sml::Worksheet>()?;
        let Some(node) = view.child::<sml::PageMargins>() else { return Ok(Margins::default()) };
        let defaults = Margins::default();
        let read = |name, fallback| node.raw().attr(q(name))
            .and_then(ooxml_core::value::parse_f64).unwrap_or(fallback);
        Ok(Margins {
            left: read("left", defaults.left),
            right: read("right", defaults.right),
            top: read("top", defaults.top),
            bottom: read("bottom", defaults.bottom),
            header: read("header", defaults.header),
            footer: read("footer", defaults.footer),
        })
    }

    /// Writes all six `pageMargins` (inches, each at least 0).
    pub fn set_margins(&mut self, m: &Margins) -> Result<()> {
        let all = [("left", m.left), ("right", m.right), ("top", m.top), ("bottom", m.bottom), ("header", m.header), ("footer", m.footer)];
        if let Some((n, v)) = all.iter().find(|(_, v)| !(*v >= 0.0 && v.is_finite())) {
            return Err(setting("margin", format!("{n} = {v}")));
        }
        self.wb.edit_sheet_child::<sml::PageMargins>(self.sheet, |pm| {
            for (n, v) in all {
                pm.set_attr(q(n), &num(v))?;
            }
            Ok(())
        })
    }

    /// Reads print options, defaulting missing attributes to false.
    pub fn print_options(&self) -> Result<PrintOptions> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let view = doc.lens::<sml::Worksheet>()?;
        let Some(node) = view.child::<sml::PrintOptions>() else {
            return Ok(PrintOptions::default());
        };
        let read = |name| node.raw().attr(q(name)).and_then(ooxml_core::value::parse_bool).unwrap_or(false);
        Ok(PrintOptions {
            gridlines: read("gridLines"),
            headings: read("headings"),
            horizontal_centered: read("horizontalCentered"),
            vertical_centered: read("verticalCentered"),
        })
    }

    /// `printOptions` (`true` writes `1`; `false` removes the attribute).
    pub fn set_print_options(&mut self, o: &PrintOptions) -> Result<()> {
        let o = *o;
        self.wb.edit_sheet_child::<sml::PrintOptions>(self.sheet, |p| {
            for (n, v) in [("horizontalCentered", o.horizontal_centered), ("verticalCentered", o.vertical_centered), ("headings", o.headings), ("gridLines", o.gridlines)] {
                put(p, n, v.then_some("1"))?
            }
            Ok(())
        })
    }

    /// Reads the worksheet header and footer texts and page flags.
    pub fn header_footer(&self) -> Result<HeaderFooter> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let view = doc.lens::<sml::Worksheet>()?;
        let Some(node) = view.child::<sml::HeaderFooter>() else {
            return Ok(HeaderFooter::default());
        };
        let text = |name| node.raw().elements().find(|child| child.is(name))
            .map(|child| child.text().into_owned());
        let flag = |name| node.raw().attr(q(name)).and_then(ooxml_core::value::parse_bool).unwrap_or(false);
        Ok(HeaderFooter {
            odd_header: text(sml::OddHeader::QNAME),
            odd_footer: text(sml::OddFooter::QNAME),
            even_header: text(sml::EvenHeader::QNAME),
            even_footer: text(sml::EvenFooter::QNAME),
            first_header: text(sml::FirstHeader::QNAME),
            first_footer: text(sml::FirstFooter::QNAME),
            different_odd_even: flag("differentOddEven"),
            different_first: flag("differentFirst"),
        })
    }

    /// Replaces the header and footer texts (ADR-0014 §10). Each text is
    /// validated first ([`Error::InvalidHeaderFooter`]).
    pub fn set_header_footer(&mut self, hf: &HeaderFooter) -> Result<()> {
        self.update_header_footer(hf, u8::MAX)
    }

    /// Updates selected entries in odd, even, first (header then footer) order,
    /// followed by the different-odd-even and different-first flags.
    pub fn update_header_footer(&mut self, hf: &HeaderFooter, changed: u8) -> Result<()> {
        if changed == 0 {
            return Ok(());
        }
        let texts = [
            (&hf.odd_header, sml::OddHeader::QNAME),
            (&hf.odd_footer, sml::OddFooter::QNAME),
            (&hf.even_header, sml::EvenHeader::QNAME),
            (&hf.even_footer, sml::EvenFooter::QNAME),
            (&hf.first_header, sml::FirstHeader::QNAME),
            (&hf.first_footer, sml::FirstFooter::QNAME),
        ];
        let is_changed = |index: usize| changed & (1 << index) != 0;
        for (index, (t, _)) in texts.iter().enumerate() {
            if is_changed(index) && let Some(t) = t {
                check_header_footer(t)?;
                if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t) {
                    return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
                }
            }
        }
        let hf = hf.clone();
        self.wb.edit_sheet_child::<sml::HeaderFooter>(self.sheet, move |h| {
            if is_changed(6) {
                put(h, "differentOddEven", hf.different_odd_even.then_some("1"))?;
            }
            if is_changed(7) {
                put(h, "differentFirst", hf.different_first.then_some("1"))?;
            }
            let doomed: Vec<usize> = h
                .node()
                .indexed_elements()
                .filter(|(_, e)| texts.iter().enumerate().any(|(index, (_, name))| is_changed(index) && e.is(*name)))
                .map(|(i, _)| i)
                .collect();
            for i in doomed.into_iter().rev() {
                h.remove_at(i);
            }
            macro_rules! put {
                ($index:expr, $t:expr, $ty:ty) => {
                    if is_changed($index) && let Some(t) = $t {
                        let mut e = h.insert::<$ty>()?;
                        if t.starts_with(char::is_whitespace) || t.ends_with(char::is_whitespace) {
                            e.set_attr(ooxml_core::name::XML_SPACE, "preserve")?;
                        }
                        e.set_text(&t)?;
                    }
                };
            }
            put!(0, hf.odd_header, sml::OddHeader);
            put!(1, hf.odd_footer, sml::OddFooter);
            put!(2, hf.even_header, sml::EvenHeader);
            put!(3, hf.even_footer, sml::EvenFooter);
            put!(4, hf.first_header, sml::FirstHeader);
            put!(5, hf.first_footer, sml::FirstFooter);
            Ok(())
        })
    }

    /// Returns worksheet row page breaks in document order.
    pub fn row_page_breaks(&self) -> Result<Vec<RowPageBreak>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let worksheet = doc.lens::<sml::Worksheet>()?;
        let Some(group) = worksheet.child::<sml::RowBreaks>() else {
            return Ok(Vec::new());
        };
        Ok(sml::RowBreaksView(group).break_iter().filter_map(|entry| {
            Some(RowPageBreak {
                id: entry.id().ok()?,
                min: entry.min().ok()?,
                max: entry.max().ok()?,
                manual: entry.manual_page_break().ok()?,
            })
        }).collect())
    }

    /// Adds a manual page break before row `row` (0-based; the break's
    /// `id`); `Ok(false)` if it exists. `count` and `manualBreakCount` are
    /// updated.
    pub fn add_row_break(&mut self, row: u32) -> Result<bool> {
        if row == 0 || row >= MAX_ROWS {
            return Err(setting("row break", format!("{row} is not inside the sheet")));
        }
        self.add_break::<sml::RowBreaks>(row, 0, MAX_COLS - 1)
    }

    /// Adds a manual row page break with inclusive orthogonal bounds.
    pub fn add_row_break_span(&mut self, row: u32, first: u32, last: u32) -> Result<bool> {
        if row == 0 || row >= MAX_ROWS || first > last {
            return Err(setting("row break", "invalid row or column span"));
        }
        self.add_break::<sml::RowBreaks>(row, first, last)
    }

    /// Adds a manual page break before column `col` (0-based).
    pub fn add_col_break(&mut self, col: u32) -> Result<bool> {
        if col == 0 || col >= MAX_COLS {
            return Err(setting("column break", format!("{col} is not inside the sheet")));
        }
        self.add_break::<sml::ColumnBreaks>(col, 0, MAX_ROWS - 1)
    }

    fn add_break<E>(&mut self, id: u32, min: u32, max: u32) -> Result<bool>
    where
        E: ooxml_core::ChildOf<sml::Worksheet>,
        sml::Break: ooxml_core::ChildOf<E>,
    {
        let part = self.wb.sheet_part_of(self.sheet);
        {
            let doc = self.wb.pkg.xml_part(&part)?;
            let ws = doc.lens::<sml::Worksheet>()?;
            let groups = || ws.raw().elements().filter(|e| e.is(E::QNAME));
            let exists = groups().flat_map(|c| c.elements()).any(|b| {
                b.is(sml::Break::QNAME) && b.attr(q("id")).and_then(|v| v.trim().parse::<u32>().ok()) == Some(id)
            });
            if exists {
                self.wb.check_editable()?;
                return Ok(false);
            }
            if let Some(group) = groups().next() {
                // The insertion index and counts are computed from direct breaks.
                refuse_alternate_content(group, E::QNAME)?;
            }
        }
        self.wb.edit_sheet_child::<E>(self.sheet, |c| {
            let pos = c
                .node()
                .indexed_elements()
                .find(|(_, b)| b.is(sml::Break::QNAME) && b.attr(q("id")).and_then(|v| v.trim().parse::<u32>().ok()).is_some_and(|x| x > id))
                .map_or(c.node().children().len(), |(i, _)| i);
            let mut b = c.insert_at::<sml::Break>(pos)?;
            b.set_attr(q("id"), &id.to_string())?;
            if min != 0 {
                b.set_attr(q("min"), &min.to_string())?;
            }
            b.set_attr(q("max"), &max.to_string())?;
            b.set_attr(q("man"), "1")?;
            let n = c.node().elements().filter(|e| e.is(sml::Break::QNAME)).count();
            let manual = c.node().elements().filter(|e| e.is(sml::Break::QNAME) && matches!(e.attr(q("man")), Some("1" | "true"))).count();
            c.set_attr(q("count"), &n.to_string())?;
            c.set_attr(q("manualBreakCount"), &manual.to_string())?;
            Ok(())
        })?;
        Ok(true)
    }
}
