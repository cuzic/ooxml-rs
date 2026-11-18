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
use crate::worksheet::Worksheet;

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

impl Workbook {
    fn sheet_part_of(&self, sheet: usize) -> String {
        self.sheets[sheet].part.clone().expect("worksheet")
    }

    /// The sheet view with `workbookViewId="0"`, as (sheetViews present,
    /// item index of the view, its node) — read only.
    fn view0(&mut self, sheet: usize) -> Result<(bool, Option<usize>, Option<RawNode>)> {
        let part = self.sheet_part_of(sheet);
        let doc = self.pkg.xml_part(&part)?;
        let ws = doc.lens::<sml::Worksheet>()?;
        let Some(views) = ws.child::<sml::SheetViews>() else { return Ok((false, None, None)) };
        let found = views
            .raw()
            .indexed_elements()
            .find(|(_, e)| e.is(sml::SheetViewElement::QNAME) && e.attr(q("workbookViewId")).is_some_and(|v| v.trim() == "0"))
            .map(|(i, e)| (i, e.clone()));
        Ok((true, found.as_ref().map(|f| f.0), found.map(|f| f.1)))
    }

    /// Edits the `workbookViewId="0"` view (created, with `sheetViews`, in
    /// their slots when absent): `plan` checks it read-only, `apply` edits it.
    fn with_view0(
        &mut self,
        sheet: usize,
        plan: impl FnOnce(Option<&RawNode>) -> Result<()>,
        apply: impl FnOnce(&mut LensMut<'_, sml::SheetViewElement>) -> Result<()>,
    ) -> Result<()> {
        self.check_editable()?;
        let (has_views, item, node) = self.view0(sheet)?;
        plan(node.as_ref())?;
        let part = self.sheet_part_of(sheet);
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match (has_views, item) {
                (false, _) => ws.check_insert_by::<sml::SheetViews>(|_, _| false)?,
                (true, None) => ws.child_mut::<sml::SheetViews>().expect("present").check_insert::<sml::SheetViewElement>()?,
                (true, Some(_)) => {}
            }
        }
        self.poisoning_layout(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut views = ws.get_or_insert::<sml::SheetViews>()?;
            let mut view = match item {
                Some(i) => views.into_child_at::<sml::SheetViewElement>(i).expect("planned"),
                None => {
                    let mut v = views.insert::<sml::SheetViewElement>()?;
                    v.set_attr(q("workbookViewId"), "0")?;
                    let i = *v.path().last().expect("child");
                    views.into_child_at::<sml::SheetViewElement>(i).expect("inserted")
                }
            };
            apply(&mut view)
        })
    }

    /// ADR-0003 §3.1 poisoning around an apply step (Phase4's fault point).
    pub(crate) fn poisoning_layout(&mut self, apply: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        self.poisoned = Some("a range-feature edit was interrupted while being applied".into());
        let result = self.fault_point(crate::workbook::ApplyStep::RangeFeature).and_then(|()| apply(self));
        match result {
            Ok(()) => {
                self.poisoned = None;
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                self.poisoned = Some(msg.clone());
                Err(Error::Poisoned(msg))
            }
        }
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
        self.wb.with_view0(
            sheet,
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
        if x_twips == 0 && y_twips == 0 {
            return Err(Error::NothingToFreeze);
        }
        let sheet = self.sheet;
        let carried_cell = std::cell::RefCell::new((None, None));
        let (x, y) = (x_twips > 0, y_twips > 0);
        self.wb.with_view0(
            sheet,
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
        let (_, _, node) = self.wb.view0(sheet)?;
        if !node.as_ref().is_some_and(|v| v.elements().any(|e| e.is(sml::Pane::QNAME))) {
            self.wb.check_editable()?;
            return Ok(false);
        }
        let cell = carried_selection(node.as_ref());
        self.wb.with_view0(
            sheet,
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
        self.wb.with_view0(self.sheet, |_| Ok(()), |v| Ok(v.set_attr(q("zoomScale"), &percent.to_string())?))
    }

    /// `showGridLines` (default shown; `false` writes `0`).
    pub fn set_show_gridlines(&mut self, show: bool) -> Result<()> {
        self.wb.with_view0(self.sheet, |_| Ok(()), |v| put(v, "showGridLines", (!show).then_some("0")))
    }

    /// `rightToLeft` (default off).
    pub fn set_right_to_left(&mut self, on: bool) -> Result<()> {
        self.wb.with_view0(self.sheet, |_| Ok(()), |v| put(v, "rightToLeft", on.then_some("1")))
    }
}

// ---- columns and rows (§9) ----

/// What a column setter changes.
#[derive(Clone, Copy, Debug)]
enum ColEdit {
    Width(f64),
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
            if cursor < m {
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
        if cursor <= b {
            let next = existing.iter().find(|e| e.1 > b).map(|e| e.0);
            let mut at = vec![(q("min"), cursor.to_string()), (q("max"), b.to_string()), (q("width"), num(default_width))];
            apply_edit(&mut at, edit);
            let col = NewCol { min: cursor, max: b, attrs: ordered(at), edit_after: false };
            ops.push(match next {
                Some(i) => Op::InsertBefore(i, col),
                None => Op::Append(col),
            });
        }
        let outline = match edit {
            ColEdit::Outline(l) => Some(l),
            _ => None,
        };
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if ws.child_mut::<sml::Columns>().is_none() {
                ws.check_insert_by::<sml::Columns>(|_, _| false)?;
            }
            if outline.is_some_and(|l| l > 0) && ws.child_mut::<sml::SheetFormatProperties>().is_none() {
                ws.check_insert_by::<sml::SheetFormatProperties>(|_, _| false)?;
            }
        }
        // ---- apply ----
        self.poisoning_layout(|wb| {
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
        let has_sd = self.indexes.get(&sheet).expect("built").sheet_data;
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if !has_sd {
                ws.check_insert_by::<sml::SheetData>(|_, _| false)?;
            } else if loc.row.is_none() {
                ws.child_mut::<sml::SheetData>().expect("indexed").check_insert_at::<sml::Row>(loc.row_insert_at)?;
            }
            if matches!(edit, RowEdit::Outline(l) if l > 0) && ws.child_mut::<sml::SheetFormatProperties>().is_none() {
                ws.check_insert_by::<sml::SheetFormatProperties>(|_, _| false)?;
            }
        }
        self.poisoning_layout(|wb| {
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
        ColEdit::Hidden(h) => put(e, "hidden", h.then_some("1"))?,
        ColEdit::Outline(l) => put(e, "outlineLevel", (l > 0).then(|| l.to_string()).as_deref())?,
    }
    Ok(())
}

/// What a row setter changes.
#[derive(Clone, Copy, Debug)]
enum RowEdit {
    Height(f64),
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
    /// Sets the width (Excel character units, 0–255, `customWidth="1"`) of
    /// columns `first..=last` (0-based), splitting a `col` range that
    /// covers them only in part (ADR-0014 §9). Errors:
    /// [`Error::AmbiguousColumns`], [`Error::InvalidSetting`].
    pub fn set_column_width(&mut self, first: u32, last: u32, width: f64) -> Result<()> {
        self.wb.set_columns(self.sheet, first, last, ColEdit::Width(width))
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

    /// Sets row `row`'s height in points (0–409, `customHeight="1"`); an
    /// empty row is created in order when needed.
    pub fn set_row_height(&mut self, row: u32, height: f64) -> Result<()> {
        self.wb.set_row(self.sheet, row, RowEdit::Height(height))
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
                ws.check_insert_by::<E>(|_, _| false)?;
            }
        }
        self.poisoning_layout(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut e = ws.get_or_insert::<E>()?;
            apply(&mut e)
        })
    }
}

impl Worksheet<'_> {
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
                        pr.check_insert_by::<sml::PageSetupProperties>(|_, _| false)?;
                    }
                }
                None if fit => ws.check_insert_by::<sml::SheetProperties>(|_, _| false)?,
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
            self.wb.poisoning_layout(|wb| {
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

    /// Replaces the header and footer texts (ADR-0014 §10). Each text is
    /// validated first ([`Error::InvalidHeaderFooter`]).
    pub fn set_header_footer(&mut self, hf: &HeaderFooter) -> Result<()> {
        let texts = [
            (&hf.odd_header, sml::OddHeader::QNAME),
            (&hf.odd_footer, sml::OddFooter::QNAME),
            (&hf.even_header, sml::EvenHeader::QNAME),
            (&hf.even_footer, sml::EvenFooter::QNAME),
            (&hf.first_header, sml::FirstHeader::QNAME),
            (&hf.first_footer, sml::FirstFooter::QNAME),
        ];
        for (t, _) in &texts {
            if let Some(t) = t {
                check_header_footer(t)?;
                if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t) {
                    return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
                }
            }
        }
        let hf = hf.clone();
        self.wb.edit_sheet_child::<sml::HeaderFooter>(self.sheet, move |h| {
            put(h, "differentOddEven", hf.different_odd_even.then_some("1"))?;
            put(h, "differentFirst", hf.different_first.then_some("1"))?;
            let doomed: Vec<usize> = h
                .node()
                .indexed_elements()
                .filter(|(_, e)| [sml::OddHeader::QNAME, sml::OddFooter::QNAME, sml::EvenHeader::QNAME, sml::EvenFooter::QNAME, sml::FirstHeader::QNAME, sml::FirstFooter::QNAME].iter().any(|n| e.is(*n)))
                .map(|(i, _)| i)
                .collect();
            for i in doomed.into_iter().rev() {
                h.remove_at(i);
            }
            macro_rules! put {
                ($t:expr, $ty:ty) => {
                    if let Some(t) = $t {
                        let mut e = h.insert::<$ty>()?;
                        if t.starts_with(char::is_whitespace) || t.ends_with(char::is_whitespace) {
                            e.set_attr(ooxml_core::name::XML_SPACE, "preserve")?;
                        }
                        e.set_text(&t)?;
                    }
                };
            }
            put!(hf.odd_header, sml::OddHeader);
            put!(hf.odd_footer, sml::OddFooter);
            put!(hf.even_header, sml::EvenHeader);
            put!(hf.even_footer, sml::EvenFooter);
            put!(hf.first_header, sml::FirstHeader);
            put!(hf.first_footer, sml::FirstFooter);
            Ok(())
        })
    }

    /// Adds a manual page break before row `row` (0-based; the break's
    /// `id`); `Ok(false)` if it exists. `count` and `manualBreakCount` are
    /// updated.
    pub fn add_row_break(&mut self, row: u32) -> Result<bool> {
        if row == 0 || row >= MAX_ROWS {
            return Err(setting("row break", format!("{row} is not inside the sheet")));
        }
        self.add_break::<sml::RowBreaks>(row, MAX_COLS - 1)
    }

    /// Adds a manual page break before column `col` (0-based).
    pub fn add_col_break(&mut self, col: u32) -> Result<bool> {
        if col == 0 || col >= MAX_COLS {
            return Err(setting("column break", format!("{col} is not inside the sheet")));
        }
        self.add_break::<sml::ColumnBreaks>(col, MAX_ROWS - 1)
    }

    fn add_break<E>(&mut self, id: u32, max: u32) -> Result<bool>
    where
        E: ooxml_core::ChildOf<sml::Worksheet>,
        sml::Break: ooxml_core::ChildOf<E>,
    {
        let part = self.wb.sheet_part_of(self.sheet);
        let exists = {
            let doc = self.wb.pkg.xml_part(&part)?;
            let ws = doc.lens::<sml::Worksheet>()?;
            ws.raw().elements().filter(|e| e.is(E::QNAME)).flat_map(|c| c.elements()).any(|b| b.is(sml::Break::QNAME) && b.attr(q("id")).and_then(|v| v.trim().parse::<u32>().ok()) == Some(id))
        };
        if exists {
            self.wb.check_editable()?;
            return Ok(false);
        }
        self.wb.edit_sheet_child::<E>(self.sheet, |c| {
            let pos = c
                .node()
                .indexed_elements()
                .find(|(_, b)| b.is(sml::Break::QNAME) && b.attr(q("id")).and_then(|v| v.trim().parse::<u32>().ok()).is_some_and(|x| x > id))
                .map_or(c.node().children().len(), |(i, _)| i);
            let mut b = c.insert_at::<sml::Break>(pos)?;
            b.set_attr(q("id"), &id.to_string())?;
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
