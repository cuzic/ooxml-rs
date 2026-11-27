//! The shared-string registry (ADR-0006 §4).
//!
//! - Activated by the first shared-string write in a workbook that has a
//!   `sharedStrings.xml` part; without one, strings stay inline (§4.6).
//! - Append-only: a new string is a new last `si`.
//! - Exact-form reuse: an existing `si` whose bytes are exactly what would
//!   be appended is reused; rich and phonetic items never match.
//! - `uniqueCount` = entries; `count` = `t="s"` cells in every sheet part
//!   the census scans (worksheets, macro sheets, dialog sheets; review n1),
//!   counted at activation and kept current by every later write. An
//!   absent attribute stays absent.

use std::collections::HashMap;

use ooxml_core::{ChildOf, ElementSpec, LensMut, Package, PartNameRequest, QName, RawNode, RegistryPlan, Target, XmlDocument};
use ooxml_schema::sml;

use super::format::{Font, Underline};
use super::{Table, census_sheet_parts, current_document, render, scan_sheet};
use crate::error::{Error, Result};
use crate::workbook::Workbook;

/// The registry state.
#[derive(Clone, Debug)]
pub(crate) struct SharedStrings {
    pub(crate) part: String,
    prefix: Option<String>,
    entries: u32,
    reuse: HashMap<String, u32>,
    /// `t="s"` cells in the workbook.
    pub(crate) refs: i64,
    has_count: bool,
    has_unique: bool,
}

/// One run of a rich string: its font (`None` = no `rPr`) and its text,
/// already `_xHHHH_`-escaped.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RichRun {
    pub(crate) font: Option<Font>,
    pub(crate) escaped: String,
}

/// The content of a new `si` (ADR-0006 §4.2): plain text or runs. The two
/// never collide: their canonical bytes differ (`si/t` against `si/r`).
#[derive(Clone, Debug)]
pub(crate) enum SiContent {
    Plain(String),
    Rich(Vec<RichRun>),
}

/// What one string write does to the registry.
#[derive(Clone, Debug)]
pub(crate) struct StringPlan {
    pub(crate) index: u32,
    /// The canonical bytes, when a new `si` is appended.
    append: Option<String>,
    content: SiContent,
    pub(crate) decoded: String,
}

impl StringPlan {
    pub(crate) fn reservation(&self) -> Option<(String, u32)> {
        self.append.as_ref().map(|key| (key.clone(), self.index))
    }

    pub(crate) fn will_append(&self) -> bool {
        self.append.is_some()
    }
}

fn build_si(parent: &mut LensMut<'_, sml::SharedStringTable>, content: &SiContent) -> Result<()> {
    let mut si = parent.insert::<sml::SharedStringItem>()?;
    match content {
        SiContent::Plain(escaped) => {
            si.insert::<sml::Text>()?.set_text(escaped)?;
        }
        SiContent::Rich(runs) => {
            for r in runs {
                build_run(&mut si, r)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn render_rich_item(runs: Vec<RichRun>) -> Result<String> {
    render::<sml::SharedStringTable>(None, |parent| build_si(parent, &SiContent::Rich(runs)))
}

pub(crate) fn render_inline_rich(runs: &[RichRun]) -> Result<String> {
    render::<sml::Cell>(None, |parent| {
        let mut inline = parent.insert::<sml::InlineString>()?;
        for run in runs {
            build_run(&mut inline, run)?;
        }
        Ok(())
    })
}

/// Appends one `r` (with `rPr` when the run has a font, then `t`) under
/// `si` or `is`.
pub(crate) fn build_run<P>(parent: &mut LensMut<'_, P>, run: &RichRun) -> Result<()>
where
    P: ElementSpec,
    sml::Run: ChildOf<P>,
{
    let mut r = parent.insert::<sml::Run>()?;
    if let Some(f) = &run.font {
        build_rpr(&mut r, f)?;
    }
    r.insert::<sml::Text>()?.set_text(&run.escaped)?;
    Ok(())
}

/// The run properties of a font (`CT_RPrElt`), in Excel's and
/// rust_xlsxwriter's order: `b`, `i`, `strike`, `condense`, `extend`, `u`,
/// `vertAlign`, `sz`, `color`, `rFont`, `family`, `charset`, `scheme`.
fn build_rpr(r: &mut LensMut<'_, sml::Run>, f: &Font) -> Result<()> {
    let mut p = r.insert::<sml::RunProperties>()?;
    let val = QName::new("", "val");
    if f.bold {
        p.insert::<sml::Bold>()?;
    }
    if f.italic {
        p.insert::<sml::Italic>()?;
    }
    if f.strike {
        p.insert::<sml::Strike>()?;
    }
    if f.condense {
        p.insert::<sml::Condense>()?;
    }
    if f.extend {
        p.insert::<sml::Extend>()?;
    }
    if let Some(u) = f.underline {
        let mut e = p.insert::<sml::Underline>()?;
        match u {
            Underline::Single => {}
            Underline::Double => e.set_attr(val, "double")?,
            Underline::SingleAccounting => e.set_attr(val, "singleAccounting")?,
            Underline::DoubleAccounting => e.set_attr(val, "doubleAccounting")?,
        }
    }
    if let Some(v) = f.vert_align {
        p.insert::<sml::VerticalTextAlignment>()?.set_attr(val, v.as_str())?;
    }
    p.insert::<sml::FontSize>()?.set_attr(val, &f.size.to_string())?;
    if let Some(c) = &f.color {
        let mut e = p.insert::<sml::Color>()?;
        for (k, v) in c.attrs() {
            e.set_attr(QName::new("", k), &v)?;
        }
    }
    p.insert::<sml::RunFont>()?.set_attr(val, &f.name)?;
    if let Some(fam) = f.family {
        p.insert::<sml::FontFamily>()?.set_attr(val, &fam.to_string())?;
    }
    if let Some(cs) = f.charset {
        p.insert::<sml::RunPropertyCharSet>()?.set_attr(val, &cs.to_string())?;
    }
    if let Some(sc) = &f.scheme {
        p.insert::<sml::FontScheme>()?.set_attr(val, sc)?;
    }
    Ok(())
}

impl SharedStrings {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries == 0 && self.refs == 0
    }

    /// Activates the registry if the workbook has a shared-string part.
    pub(crate) fn activate(wb: &mut Workbook) -> Result<Option<SharedStrings>> {
        let rels = wb.pkg.relationships(&wb.workbook_part)?;
        let Some(part) = rels
            .by_type(crate::workbook::REL_SHARED_STRINGS)
            .next()
            .and_then(|r| rels.resolve(r))
            .filter(|p| wb.pkg.contains(p))
        else {
            return Ok(None);
        };
        let mut refs = 0i64;
        for p in census_sheet_parts(wb) {
            refs += scan_sheet(&wb.pkg.read_part(&p)?)?.shared as i64;
        }
        let doc = current_document(wb.pkg.xml_part(&part)?)?;
        let root = doc.root();
        if !root.is(sml::SharedStringTable::QNAME) {
            return Err(Error::Malformed(format!("{part}: root is not sst")));
        }
        let t = Table::read(&doc, Some(root), sml::SharedStringItem::QNAME);
        if t.has_ac {
            // indices behind an mc:AlternateContent depend on the reader
            return Err(ooxml_core::Error::UnresolvedSibling(sml::SharedStringTable::QNAME).into());
        }
        Ok(Some(SharedStrings {
            prefix: root.name().prefix().map(str::to_owned),
            entries: t.count,
            reuse: std::sync::Arc::unwrap_or_clone(t.reuse),
            refs,
            has_count: root.attr(QName::new("", "count")).is_some(),
            has_unique: root.attr(QName::new("", "uniqueCount")).is_some(),
            part,
        }))
    }

    /// Plans one plain string (already `_xHHHH_`-escaped).
    pub(crate) fn plan(&self, escaped: &str, decoded: &str) -> Result<StringPlan> {
        self.plan_content(SiContent::Plain(escaped.into()), decoded)
    }

    /// Plans one string, plain or rich. An existing `si` is reused only if
    /// its bytes are exactly the canonical form of `content` (§4.3).
    pub(crate) fn plan_content(&self, content: SiContent, decoded: &str) -> Result<StringPlan> {
        let key = render::<sml::SharedStringTable>(self.prefix.as_deref(), |p| build_si(p, &content))?;
        if let Some(&i) = self.reuse.get(&key) {
            return Ok(StringPlan { index: i, append: None, content, decoded: decoded.into() });
        }
        Ok(StringPlan { index: self.entries, append: Some(key), content, decoded: decoded.into() })
    }

    /// Assigns new slots in write order and reuses earlier batch appends.
    pub(crate) fn reserve(&self, plan: &mut StringPlan, appends: &mut HashMap<String, u32>) {
        if let Some(key) = &plan.append {
            if let Some(&index) = appends.get(key) {
                plan.index = index;
                plan.append = None;
            } else {
                plan.index = self.entries + appends.len() as u32;
                appends.insert(key.clone(), plan.index);
            }
        }
    }

    /// Applies a plan and a change in the number of `t="s"` cells. Called
    /// from the apply phase; a failure poisons the workbook.
    pub(crate) fn apply(wb: &mut Workbook, plans: &[StringPlan], refs_delta: i64, refs_changed: bool) -> Result<()> {
        if plans.is_empty() && !refs_changed {
            return Ok(());
        }
        let Some(mut reg) = wb.registries.strings.take() else { return Ok(()) };
        let result = (|| -> Result<()> {
            let doc = wb.pkg.xml_part_mut(&reg.part)?;
            let mut sst = doc.lens_mut::<sml::SharedStringTable>()?;
            for p in plans {
                let key = p.append.as_ref().expect("new shared string");
                debug_assert_eq!(p.index, reg.entries);
                build_si(&mut sst, &p.content)?;
                reg.reuse.insert(key.clone(), reg.entries);
                reg.entries += 1;
                if let Some(cache) = wb.shared_strings.as_mut() {
                    cache.push(p.decoded.clone());
                }
            }
            if !plans.is_empty() && reg.has_unique {
                sst.set_attr(QName::new("", "uniqueCount"), &reg.entries.to_string())?;
            }
            reg.refs += refs_delta;
            if reg.has_count {
                sst.set_attr(QName::new("", "count"), &reg.refs.max(0).to_string())?;
            }
            Ok(())
        })();
        wb.registries.strings = Some(reg);
        result
    }
}

fn sheet_cell_paths(doc: &XmlDocument, predicate: impl Fn(&RawNode) -> bool) -> Vec<[usize; 3]> {
    let mut paths = Vec::new();
    for (data_index, data) in doc.root().indexed_elements().filter(|(_, data)| data.is(sml::SheetData::QNAME)) {
        for (row_index, row) in data.indexed_elements().filter(|(_, row)| row.is(sml::Row::QNAME)) {
            paths.extend(row.indexed_elements()
                .filter(|(_, cell)| cell.is(sml::Cell::QNAME) && predicate(cell))
                .map(|(cell_index, _)| [data_index, row_index, cell_index]));
        }
    }
    paths
}

fn shared_string_part(package: &Package, workbook_part: &str) -> Result<Option<String>> {
    let rels = package.relationships(workbook_part)?;
    let mut matches = rels.by_type(crate::workbook::REL_SHARED_STRINGS);
    let Some(relationship) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(Error::Malformed("multiple shared string relationships".into()));
    }
    rels.resolve(relationship).map(Some)
        .ok_or_else(|| Error::Malformed("invalid shared string relationship".into()))
}

/// Exports shared strings as cell-local values in a separate package.
pub(crate) fn unshare(
    wb: &Workbook,
    package: &Package,
    plain_as_value: bool,
) -> Result<Option<Package>> {
    let Some(part) = shared_string_part(package, &wb.workbook_part)? else {
        return Ok(None);
    };
    wb.check_editable()?;
    let strings = XmlDocument::parse(&package.read_part(&part)?)?;
    strings.lens::<sml::SharedStringTable>()?;
    let table = Table::read(&strings, Some(strings.root()), sml::SharedStringItem::QNAME);
    if table.has_ac {
        return Err(ooxml_core::Error::UnresolvedSibling(sml::SharedStringTable::QNAME).into());
    }
    let items: Vec<_> = strings.root().indexed_elements()
        .filter(|(_, item)| item.is(sml::SharedStringItem::QNAME))
        .map(|(index, _)| strings.lens_at::<sml::SharedStringItem>(&[index]))
        .collect::<std::result::Result<_, _>>()?;
    let mut output = package.fork();
    for sheet in census_sheet_parts(wb) {
        let bytes = package.read_part(&sheet)?;
        let expected = scan_sheet(&bytes)?.shared;
        if expected == 0 {
            continue;
        }
        let mut doc = XmlDocument::parse(&bytes)?;
        let paths = sheet_cell_paths(&doc, |cell| cell.attr(sml::Cell::ATTR_T) == Some("s"));
        if paths.len() as u64 != expected {
            return Err(Error::Malformed(format!("{sheet}: shared string cell outside sheetData/row")));
        }
        for path in paths {
            let cell = doc.lens_at::<sml::Cell>(&path)?;
            let index = cell.child::<sml::CellValue>()
                .and_then(|value| value.text().trim().parse::<usize>().ok())
                .filter(|index| *index < items.len())
                .ok_or_else(|| Error::Malformed(format!("{sheet}: invalid shared string index")))?;
            let item = &items[index];
            let mut cell = sml::CellViewMut(doc.lens_mut_at::<sml::Cell>(&path)?);
            if plain_as_value && item.raw().attrs().is_empty()
                && let [child] = item.raw().elements().collect::<Vec<_>>().as_slice()
                && child.is(sml::Text::QNAME)
            {
                cell.set_data_type(sml::CellValues::String)?;
                cell.get_or_insert_cell_value()?.0.set_text(&child.text())?;
            } else {
                let inline = item.detach_as::<sml::InlineString>(&[])?;
                cell.remove_cell_value();
                cell.set_data_type(sml::CellValues::InlineString)?;
                cell.0.insert_detached(&inline)?;
            }
        }
        output.set_part(&sheet, doc.serialize().into_bytes())?;
    }
    let mut plan = RegistryPlan::new(&output);
    plan.remove_parts(&output, &[&part], &[crate::workbook::REL_SHARED_STRINGS])?;
    output.apply(plan)?;
    Ok(Some(output))
}

const CT_SHARED_STRINGS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";

/// Exports cell-local strings as workbook-wide shared strings.
pub(crate) fn share(wb: &Workbook, package: &Package) -> Result<Option<Package>> {
    wb.check_editable()?;
    let part = shared_string_part(package, &wb.workbook_part)?;
    let mut strings = match &part {
        Some(part) => XmlDocument::parse(&package.read_part(part)?)?,
        None => XmlDocument::parse(format!("<sst xmlns=\"{}\"/>", crate::SML_NS).as_bytes())?,
    };
    strings.lens::<sml::SharedStringTable>()?;
    let table = Table::read(&strings, Some(strings.root()), sml::SharedStringItem::QNAME);
    if !table.stable || table.has_ac {
        return Err(ooxml_core::Error::UnresolvedSibling(sml::SharedStringTable::QNAME).into());
    }
    let mut entries = table.count;
    let mut reuse = std::sync::Arc::unwrap_or_clone(table.reuse);
    let prefix = strings.root().name().prefix().map(str::to_owned);
    let mut changed = Vec::new();
    let mut references = 0_u64;
    for sheet in census_sheet_parts(wb) {
        let bytes = package.read_part(&sheet)?;
        references += scan_sheet(&bytes)?.shared;
        let mut doc = XmlDocument::parse(&bytes)?;
        let paths = sheet_cell_paths(&doc, |cell| {
            matches!(cell.attr(sml::Cell::ATTR_T), Some("str" | "inlineStr"))
                && !cell.elements().any(|child| child.is(sml::CellFormula::QNAME))
        });
        if paths.is_empty() {
            continue;
        }
        let mut edited = false;
        for path in paths {
            let cell = doc.lens_at::<sml::Cell>(&path)?;
            crate::worksheet::refuse_alternate_content(cell.raw(), sml::Cell::QNAME)?;
            let inline = cell.child::<sml::InlineString>()
                .map(|value| value.detach_as::<sml::SharedStringItem>(&[])).transpose()?;
            let plain = cell.child::<sml::CellValue>().map(|value| SiContent::Plain(value.text().into_owned()));
            if inline.is_none() && plain.is_none() {
                continue;
            }
            let insert = |table: &mut LensMut<'_, sml::SharedStringTable>| {
                if let Some(value) = &inline {
                    table.insert_detached(value)?;
                } else if let Some(value) = &plain {
                    build_si(table, value)?;
                }
                Ok(())
            };
            let key = render::<sml::SharedStringTable>(prefix.as_deref(), insert)?;
            let index = if let Some(index) = reuse.get(&key) {
                *index
            } else {
                let index = entries;
                entries = entries.checked_add(1)
                    .ok_or_else(|| Error::Malformed("too many shared string entries".into()))?;
                insert(&mut strings.lens_mut::<sml::SharedStringTable>()?)?;
                reuse.insert(key, index);
                index
            };
            let mut cell = sml::CellViewMut(doc.lens_mut_at::<sml::Cell>(&path)?);
            cell.remove_inline_string();
            cell.remove_cell_value();
            cell.set_data_type(sml::CellValues::SharedString)?;
            cell.get_or_insert_cell_value()?.0.set_text(&index.to_string())?;
            references += 1;
            edited = true;
        }
        if edited {
            changed.push((sheet, doc.serialize().into_bytes()));
        }
    }
    if changed.is_empty() {
        return Ok(None);
    }
    {
        let mut table = sml::SharedStringTableViewMut(strings.lens_mut::<sml::SharedStringTable>()?);
        table.set_count(u32::try_from(references)
            .map_err(|_| Error::Malformed("too many shared string references".into()))?)?;
        table.set_unique_count(entries)?;
    }

    let mut output = package.fork();
    let table = strings.serialize().into_bytes();
    if let Some(part) = part {
        output.set_part(&part, table)?;
    } else {
        let directory = wb.workbook_part.rsplit_once('/').map_or("", |(dir, _)| dir);
        let prefix = if directory.is_empty() { String::new() } else { format!("{directory}/") };
        let canonical = format!("{prefix}sharedStrings.xml");
        let request = if output.part_name_in_use(&canonical).is_some() {
            PartNameRequest::Numbered(format!("{prefix}sharedStrings{{}}.xml"))
        } else {
            PartNameRequest::Exact(canonical)
        };
        let mut plan = RegistryPlan::new(&output);
        let part = plan.add_part(&output, request, CT_SHARED_STRINGS, table)?;
        plan.add_relationship(&output, Some(&wb.workbook_part), crate::workbook::REL_SHARED_STRINGS, Target::Part(part))?;
        output.apply(plan)?;
    }
    for (part, bytes) in changed {
        output.set_part(&part, bytes)?;
    }
    Ok(Some(output))
}
