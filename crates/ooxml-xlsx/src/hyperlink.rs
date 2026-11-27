//! Phase4 part 2: hyperlinks (ADR-0014 §4).
//!
//! An external link (URL or file) gets a new `TargetMode="External"`
//! relationship of the sheet through `ooxml-core`'s `RegistryPlan` (ids
//! never reused); an internal link is a `location` only. Order of one call
//! (§12): every check first; then the optional cell text (a Phase1 string
//! write, with its own plan); then the registry plan, built against the
//! package as it is now, and applied; then the `hyperlink` element. A
//! failure after the first change is a bug and poisons the workbook.

use ooxml_core::{ElementSpec, NsDecl, Package, QName, RegistryPlan, Target, XmlDocument};
use ooxml_schema::sml;

use crate::edit::CellOp;
use crate::error::{Error, Result};
use crate::escape;
use crate::formula::lexer::Prefix;
use crate::formula::parse::{Expr, parse};
use crate::formula::precedents::fold;
use crate::reference::{CellIndex, CellRange};
use crate::workbook::Workbook;
use crate::worksheet::Worksheet;

const REL_HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_NS_DECL: [NsDecl<'static>; 1] = [NsDecl::new("r", R_NS)];
/// rust_xlsxwriter's limit for a link target.
const MAX_URL: usize = 2080;

const fn q(local: &'static str) -> QName {
    QName::new("", local)
}

/// A hyperlink target (ADR-0014 §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// An absolute URI with a scheme (`https://…`, `mailto:…`). A
    /// `#fragment` goes to `location`, as Excel writes it.
    Url(String),
    /// A relative, UNC (`\\server\share\x.xlsx`) or drive path; `#` is
    /// escaped as `%23`.
    File(String),
    /// A place in this workbook: `Sheet2!A1`, `'My Sheet'!B2:C3`, or a
    /// defined name.
    Location(String),
}

/// Optional parts of a hyperlink.
#[derive(Clone, Debug, Default)]
pub struct LinkOptions {
    /// `display` (what Excel shows when the cell has no value).
    pub display: Option<String>,
    /// `tooltip` (at most 255 characters).
    pub tooltip: Option<String>,
    /// Also writes this string into the anchor cell (the same write as
    /// `set_string`: dependents lose their cached values).
    pub text: Option<String>,
}

/// A hyperlink as read from the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HyperlinkInfo {
    /// `ref`.
    pub range: String,
    /// The external target (`r:id` resolved), if any.
    pub target: Option<String>,
    /// `location`, if any.
    pub location: Option<String>,
    /// `display`.
    pub display: Option<String>,
    /// `tooltip`.
    pub tooltip: Option<String>,
}

impl HyperlinkInfo {
    /// The link as one address; see [`hyperlink_address`].
    pub fn address(&self) -> String {
        hyperlink_address(self.target.as_deref(), self.location.as_deref())
    }
}

/// Joins a resolved `r:id` target and a `location` as `target#location`.
///
/// Some writers (ExcelJS among them) store an in-document link such as
/// `Sheet2!B2` both as the relationship target and as `location`; when the
/// two are the same string the link is that string, not a doubled one.
pub fn hyperlink_address(target: Option<&str>, location: Option<&str>) -> String {
    let target = target.unwrap_or_default();
    match location {
        Some(location) if location != target => format!("{target}#{location}"),
        _ => target.to_owned(),
    }
}

/// rust_xlsxwriter's escaping: space, `%`, `"`, `<`, `>`, `[`, `]`, `^`,
/// `{`, `}` percent-encoded (an existing `%XX` kept); `#` too in file links.
fn escape_target(s: &str, file: bool) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    for (i, ch) in s.char_indices() {
        let kept_escape = ch == '%' && b.get(i + 1).is_some_and(u8::is_ascii_hexdigit) && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit);
        match ch {
            '%' if kept_escape => out.push('%'),
            ' ' | '%' | '"' | '<' | '>' | '[' | ']' | '^' | '{' | '}' => out.push_str(&format!("%{:02X}", ch as u32)),
            '#' if file => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out
}

fn has_scheme(s: &str) -> bool {
    s.split_once(':').is_some_and(|(sch, _)| {
        sch.len() >= 2 && sch.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && sch.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// External target (escaped) and location of a link.
fn split_link(link: &Link) -> Result<(Option<String>, Option<String>)> {
    let check = |t: &str| -> Result<()> {
        if t.is_empty() {
            return Err(Error::InvalidUrl("empty".into()));
        }
        if t.chars().count() > MAX_URL {
            return Err(Error::InvalidUrl(format!("longer than {MAX_URL} characters")));
        }
        if let Some(c) = t.chars().find(|c| c.is_whitespace() || c.is_control()) {
            return Err(Error::InvalidUrl(format!("contains U+{:04X}", c as u32)));
        }
        Ok(())
    };
    match link {
        Link::Url(u) => {
            if !has_scheme(u) {
                return Err(Error::InvalidUrl(format!("{u:?} has no scheme")));
            }
            let (base, frag) = match u.split_once('#') {
                Some((b, f)) => (b, Some(f.to_owned())),
                None => (u.as_str(), None),
            };
            let t = escape_target(base, false);
            check(&t)?;
            Ok((Some(t), frag.filter(|f| !f.is_empty())))
        }
        Link::File(f) => {
            let t = escape_target(f, true);
            check(&t)?;
            Ok((Some(t), None))
        }
        Link::Location(l) => Ok((None, Some(l.clone()))),
    }
}

impl Workbook {
    /// Lexes a `location` and checks that its sheets exist (another sheet
    /// is fine for a navigation target).
    fn check_location(&self, loc: &str) -> Result<()> {
        let e = parse(loc).map_err(|e| Error::InvalidFormula { text: loc.to_owned(), reason: format!("{} at byte {}", e.message, e.at) })?;
        let mut missing = None;
        crate::rangefmla::visit(&e, &mut |expr| {
            let p = match expr {
                Expr::Ref(r) => &r.prefix,
                Expr::Name { prefix, .. } => prefix,
                _ => return,
            };
            if let Prefix::Sheet(n) | Prefix::Sheets(n, _) = p
                && !self.sheets.iter().any(|s| fold(&s.name) == fold(n))
            {
                missing.get_or_insert_with(|| n.clone());
            }
        });
        match missing {
            Some(n) => Err(Error::UnknownSheet(n)),
            None => Ok(()),
        }
    }

    fn set_hyperlink(&mut self, sheet: usize, range: &str, link: &Link, opts: &LinkOptions) -> Result<()> {
        self.check_editable()?;
        let r = CellRange::parse(range)?;
        let (target, location) = split_link(link)?;
        if let Some(l) = &location
            && matches!(link, Link::Location(_))
        {
            self.check_location(l)?;
        }
        for t in [&location, &opts.display, &opts.tooltip].into_iter().flatten() {
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
        }
        if opts.tooltip.as_ref().is_some_and(|t| t.encode_utf16().count() > 255) {
            return Err(Error::InvalidSetting { what: "tooltip".into(), reason: "over 255 characters".into() });
        }
        let ctx = self.contexts.get(&sheet).expect("context");
        // a link in a merged range goes on its anchor (a single cell) only
        if let Some(m) = ctx.merges.iter().find(|m| m.intersects(&r) && !(r.first == r.last && r.first == m.first)) {
            return Err(Error::MergedCell { cell: r.to_string(), range: m.to_string() });
        }
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        {
            let doc = self.pkg.xml_part(&part)?;
            let ws = doc.root();
            if let Some(h) = ws.elements().find(|e| e.is(sml::Hyperlinks::QNAME)) {
                for e in h.elements().filter(|e| e.is(sml::Hyperlink::QNAME)) {
                    let existing = e.attr(q("ref")).unwrap_or_default();
                    if !CellRange::parse(existing).is_ok_and(|x| !x.intersects(&r)) {
                        return Err(Error::HyperlinkOverlap { range: r.to_string(), existing: existing.to_owned() });
                    }
                }
            }
        }
        let decls: &[_] = if target.is_some() { &REL_NS_DECL } else { &[] };
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match ws.child_mut::<sml::Hyperlinks>() {
                Some(h) => h.check_insert_with_ns::<sml::Hyperlink>(decls).map(drop)?,
                None => ws.check_insert::<sml::Hyperlinks>()?,
            }
        }
        let text_op = match &opts.text {
            Some(t) => {
                if t.encode_utf16().count() > 32_767 {
                    return Err(Error::StringTooLong(t.encode_utf16().count()));
                }
                let op = CellOp::Text(escape::encode(t).into_owned());
                self.check_edit_cell(sheet, r.first, op.clone())?;
                Some(op)
            }
            None => None,
        };
        if let Some(t) = &target {
            // the registry step is validated before the first change
            let mut plan = RegistryPlan::new(&self.pkg);
            plan.add_relationship(&self.pkg, Some(&part), REL_HYPERLINK, Target::External(t.clone()))?;
        }
        // ---- apply ----
        if let Some(op) = text_op {
            self.edit_cell(sheet, r.first, op)?;
        }
        let (display, tooltip) = (opts.display.clone(), opts.tooltip.clone());
        self.poisoning(|wb| {
            let id = match &target {
                Some(t) => Some(add_rel(&mut wb.pkg, &part, t)?),
                None => None,
            };
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut hs = ws.get_or_insert::<sml::Hyperlinks>()?;
            let mut h = hs.insert_with_ns::<sml::Hyperlink>(decls)?;
            h.set_attr(q("ref"), &r.to_string())?;
            if let Some(id) = &id {
                h.set_attr(QName::new(R_NS, "id"), id)?;
            }
            for (n, v) in [("location", &location), ("tooltip", &tooltip), ("display", &display)] {
                if let Some(v) = v {
                    h.set_attr(q(n), v)?;
                }
            }
            Ok(())
        })
    }

    fn remove_hyperlink(&mut self, sheet: usize, range: &str) -> Result<()> {
        self.check_editable()?;
        let range = CellRange::parse(range)?;
        if !self.remove_matching_hyperlinks(sheet, |reference| reference == range)? {
            return Err(Error::NoSuchRule { what: "hyperlink", sqref: range.to_string() });
        }
        Ok(())
    }

    fn remove_matching_hyperlinks(
        &mut self,
        sheet: usize,
        matches: impl Fn(CellRange) -> bool,
    ) -> Result<bool> {
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        let (items, total) = hyperlink_deletions(self.pkg.xml_part(&part)?, |_, range| {
            Ok(range.is_some_and(&matches))
        })?;
        if items.is_empty() {
            return Ok(false);
        }
        self.poisoning(|wb| apply_hyperlink_deletions(wb.pkg.xml_part_mut(&part)?, &items, total))?;
        Ok(true)
    }

    fn hyperlinks(&mut self, sheet: usize) -> Result<Vec<HyperlinkInfo>> {
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        let rels = self.pkg.relationships(&part)?;
        let doc = self.pkg.xml_part(&part)?;
        let Some(h) = doc.root().elements().find(|e| e.is(sml::Hyperlinks::QNAME)) else { return Ok(Vec::new()) };
        Ok(h.elements()
            .filter(|e| e.is(sml::Hyperlink::QNAME))
            .map(|e| HyperlinkInfo {
                range: e.attr(q("ref")).unwrap_or_default().to_owned(),
                target: e.attr(QName::new(R_NS, "id")).and_then(|id| rels.by_id(id)).map(|r| r.target.clone()),
                location: e.attr(q("location")).map(str::to_owned),
                display: e.attr(q("display")).map(str::to_owned),
                tooltip: e.attr(q("tooltip")).map(str::to_owned),
            })
            .collect())
    }
}

fn hyperlink_deletions(
    document: &XmlDocument,
    mut matches: impl FnMut(usize, Option<CellRange>) -> Result<bool>,
) -> Result<(Vec<usize>, usize)> {
    let mut items = Vec::new();
    let mut total = 0;
    if let Some(group) = document.root().elements().find(|node| node.is(sml::Hyperlinks::QNAME)) {
        for (index, element) in group.indexed_elements().filter(|(_, node)| node.is(sml::Hyperlink::QNAME)) {
            total += 1;
            let range = element.attr(q("ref")).and_then(|value| CellRange::parse(value).ok());
            if matches(index, range)? {
                items.push(index);
            }
        }
    }
    Ok((items, total))
}

fn apply_hyperlink_deletions(document: &mut XmlDocument, items: &[usize], total: usize) -> Result<()> {
    let mut worksheet = document.lens_mut::<sml::Worksheet>()?;
    if items.len() == total {
        worksheet.remove::<sml::Hyperlinks>(); // Excel repairs an empty group
    } else {
        let mut links = worksheet.child_mut::<sml::Hyperlinks>().expect("planned");
        for &index in items.iter().rev() {
            links.remove_at(index);
        }
    }
    // Keep the relationships: multiple links may share an ID.
    Ok(())
}

pub(crate) enum TableLinks {
    Insert { moved: CellRange, added: CellRange },
    InsertColumn { moved: CellRange, added: CellRange },
    Remove { deleted: CellRange, moved: Option<CellRange> },
    RemoveColumns { deleted: CellRange, moved: Option<CellRange> },
    ReplaceTotals(std::collections::HashSet<CellIndex>),
}

pub(crate) fn table_edit_hyperlinks(
    document: &XmlDocument,
    edit: TableLinks,
) -> Result<Option<XmlDocument>> {
    let mut rewritten = Vec::new();
    let (items, total) = hyperlink_deletions(document, |index, range| {
        let Some(mut range) = range else {
            return Err(Error::TableRange { reason: "table row edit encountered an unlocated hyperlink" });
        };
        let (moved, row_offset, column_offset, removed) = match edit {
            TableLinks::Insert { moved, added } => {
                if added.intersects(&range) {
                    return Err(Error::TableRange { reason: "inserted table rows would overwrite an existing hyperlink" });
                }
                (Some(moved), (added.last.row - added.first.row + 1) as i32, 0, None)
            }
            TableLinks::InsertColumn { moved, added } => {
                if added.intersects(&range) {
                    return Err(Error::TableRange { reason: "inserted table column would overwrite an existing hyperlink" });
                }
                (Some(moved), 0, 1, None)
            }
            TableLinks::Remove { deleted, moved } => {
                (moved, -((deleted.last.row - deleted.first.row + 1) as i32), 0, Some(deleted))
            }
            TableLinks::RemoveColumns { deleted, moved } => {
                (moved, 0, -((deleted.last.col - deleted.first.col + 1) as i32), Some(deleted))
            }
            TableLinks::ReplaceTotals(ref targets) => {
                if targets.len() == 1 {
                    let at = *targets.iter().next().expect("one totals cell");
                    (None, 0, 0, Some(CellRange { first: at, last: at }))
                } else if range.first == range.last {
                    return Ok(targets.contains(&range.first));
                } else if targets.iter().any(|at| range.contains(*at)) {
                    return Err(Error::TableRange { reason: "replacing totals would split a hyperlink range" });
                } else {
                    return Ok(false);
                }
            }
        };
        if let Some(moved) = moved.filter(|moved| moved.intersects(&range)) {
            if !moved.contains(range.first) || !moved.contains(range.last) {
                return Err(Error::TableRange { reason: "moving table rows would split a hyperlink range" });
            }
            for corner in [&mut range.first, &mut range.last] {
                corner.row = corner.row.checked_add_signed(row_offset).expect("validated table shift");
                corner.col = corner.col.checked_add_signed(column_offset).expect("validated table shift");
            }
            rewritten.push((index, range));
            return Ok(false);
        }
        let Some(removed) = removed else { return Ok(false) };
        if !removed.intersects(&range) {
            return Ok(false);
        }
        if removed.contains(range.first) && removed.contains(range.last) {
            return Ok(true);
        }
        if removed.first.col <= range.first.col && removed.last.col >= range.last.col {
            if range.first.row < removed.first.row && range.last.row <= removed.last.row {
                range.last.row = removed.first.row - 1;
                rewritten.push((index, range));
                return Ok(false);
            }
            if range.first.row >= removed.first.row && range.last.row > removed.last.row {
                range.first.row = removed.last.row + 1;
                rewritten.push((index, range));
                return Ok(false);
            }
        }
        if removed.first.row <= range.first.row && removed.last.row >= range.last.row {
            if range.first.col < removed.first.col && range.last.col <= removed.last.col {
                range.last.col = removed.first.col - 1;
                rewritten.push((index, range));
                return Ok(false);
            }
            if range.first.col >= removed.first.col && range.last.col > removed.last.col {
                range.first.col = removed.last.col + 1;
                rewritten.push((index, range));
                return Ok(false);
            }
        }
        Err(Error::TableRange { reason: "removed table rows would split a hyperlink range" })
    })?;
    if items.is_empty() && rewritten.is_empty() {
        return Ok(None);
    }
    let mut edited = document.clone();
    for (index, range) in rewritten {
        let mut worksheet = edited.lens_mut::<sml::Worksheet>()?;
        let mut link = worksheet.child_mut::<sml::Hyperlinks>().expect("planned")
            .into_child_at::<sml::Hyperlink>(index).expect("planned");
        link.set_attr(q("ref"), &range.to_string())?;
    }
    if !items.is_empty() {
        apply_hyperlink_deletions(&mut edited, &items, total)?;
    }
    Ok(Some(edited))
}

/// One registry plan with the sheet's new external relationship, applied.
fn add_rel(pkg: &mut Package, part: &str, target: &str) -> Result<String> {
    let mut plan = RegistryPlan::new(pkg);
    let id = plan.add_relationship(pkg, Some(part), REL_HYPERLINK, Target::External(target.to_owned()))?;
    pkg.apply(plan)?;
    Ok(id)
}

impl Worksheet<'_> {
    /// Adds a hyperlink on `range` (ADR-0014 §4). External links get a new
    /// external relationship of the sheet and declare its namespace on the
    /// new link when needed. The cell value changes only with
    /// [`LinkOptions::text`]. Refused, with every byte unchanged: overlap
    /// with a hyperlink ([`Error::HyperlinkOverlap`]), a non-anchor cell of
    /// a merged range ([`Error::MergedCell`]), a bad target
    /// ([`Error::InvalidUrl`]), an unknown sheet in a location
    /// ([`Error::UnknownSheet`]), and the text write's refusals.
    pub fn set_hyperlink(&mut self, range: &str, link: &Link, opts: &LinkOptions) -> Result<()> {
        self.wb.set_hyperlink(self.sheet, range, link, opts)
    }

    /// Updates the tooltip of one existing hyperlink without changing its target.
    pub fn set_hyperlink_tooltip(&mut self, reference: &str, tooltip: Option<&str>) -> Result<()> {
        self.wb.check_editable()?;
        let range = CellRange::parse(reference)?;
        if let Some(tooltip) = tooltip {
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(tooltip) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
            if tooltip.encode_utf16().count() > 255 {
                return Err(Error::InvalidSetting { what: "tooltip".into(), reason: "over 255 characters".into() });
            }
        }
        let part = self.part_name().to_owned();
        let index = {
            let document = self.wb.pkg.xml_part(&part)?;
            let mut matches = document.root().elements()
                .find(|node| node.is(sml::Hyperlinks::QNAME))
                .into_iter().flat_map(|group| group.elements())
                .filter(|node| node.is(sml::Hyperlink::QNAME))
                .enumerate().filter(|(_, node)| {
                    node.attr(q("ref")).and_then(|value| CellRange::parse(value).ok()) == Some(range)
                });
            let (index, _) = matches.next().ok_or_else(|| Error::NoSuchRule {
                what: "hyperlink", sqref: range.to_string(),
            })?;
            if matches.next().is_some() {
                return Err(Error::HyperlinkOverlap { range: range.to_string(), existing: range.to_string() });
            }
            index
        };
        self.wb.poisoning(|wb| {
            let mut sheet = wb.pkg.xml_part_mut(&part)?.lens_mut::<sml::Worksheet>()?;
            let mut links = sheet.child_mut::<sml::Hyperlinks>().expect("planned group");
            let mut link = links.nth_child_mut::<sml::Hyperlink>(index).expect("planned hyperlink");
            if let Some(value) = tooltip {
                link.set_attr(q("tooltip"), value)?;
            } else {
                link.remove_attr(q("tooltip"))?;
            }
            Ok(())
        })
    }

    /// Removes the hyperlinks whose `ref` equals `range`; the relationship
    /// stays (several links may share it). [`Error::NoSuchRule`] if none.
    pub fn remove_hyperlink(&mut self, range: &str) -> Result<()> {
        self.wb.remove_hyperlink(self.sheet, range)
    }

    /// Removes individual cell links in the range, leaving multi-cell links intact.
    pub fn remove_cell_hyperlinks(&mut self, range: CellRange) -> Result<()> {
        self.wb.check_editable()?;
        self.wb.remove_matching_hyperlinks(self.sheet, |reference| {
            reference.first == reference.last && range.contains(reference.first)
        }).map(drop)
    }

    /// Removes individual links at the given cells, preserving multi-cell links.
    pub fn remove_cell_hyperlinks_at(&mut self, cells: &std::collections::HashSet<CellIndex>) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }
        self.wb.check_editable()?;
        self.wb.remove_matching_hyperlinks(self.sheet, |reference| {
            reference.first == reference.last && cells.contains(&reference.first)
        }).map(drop)
    }

    /// The sheet's hyperlinks.
    pub fn hyperlinks(&mut self) -> Result<Vec<HyperlinkInfo>> {
        self.wb.hyperlinks(self.sheet)
    }
}

#[cfg(test)]
mod tests {
    use super::hyperlink_address;

    #[test]
    fn address_joins_target_and_location_once() {
        assert_eq!(hyperlink_address(Some("https://e.com/a"), None), "https://e.com/a");
        assert_eq!(hyperlink_address(Some("https://e.com/a"), Some("frag")), "https://e.com/a#frag");
        assert_eq!(hyperlink_address(None, Some("Sheet2!B2")), "#Sheet2!B2");
        assert_eq!(hyperlink_address(None, None), "");
        // A location that repeats the relationship target is one link.
        assert_eq!(hyperlink_address(Some("Sheet2!B2"), Some("Sheet2!B2")), "Sheet2!B2");
        assert_eq!(hyperlink_address(Some("#Sheet2!B2"), Some("#Sheet2!B2")), "#Sheet2!B2");
        assert_eq!(hyperlink_address(Some("#Sheet2!B2"), Some("Sheet2!B2")), "#Sheet2!B2#Sheet2!B2");
    }
}
