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

use ooxml_core::{ElementSpec, LensMut, QName};
use ooxml_schema::sml;

use super::{Table, count_shared_refs, render};
use crate::census::SheetKind;
use crate::error::{Error, Result};
use crate::workbook::Workbook;

/// The registry state.
#[derive(Debug)]
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

/// What one string write does to the registry.
#[derive(Clone, Debug)]
pub(crate) struct StringPlan {
    pub(crate) index: u32,
    /// The canonical bytes, when a new `si` is appended.
    append: Option<String>,
    escaped: String,
    decoded: String,
}

fn build_si(parent: &mut LensMut<'_, sml::SharedStringTable>, escaped: &str) -> Result<()> {
    let mut si = parent.insert::<sml::SharedStringItem>()?;
    si.insert::<sml::Text>()?.set_text(escaped)?;
    Ok(())
}

impl SharedStrings {
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
        let sheet_parts: Vec<String> = wb
            .sheets
            .iter()
            .filter(|s| matches!(s.kind, SheetKind::Worksheet | SheetKind::MacroSheet | SheetKind::DialogSheet))
            .filter_map(|s| s.part.clone())
            .collect();
        for p in sheet_parts {
            refs += count_shared_refs(&wb.pkg.read_part(&p)?)? as i64;
        }
        let doc = wb.pkg.xml_part(&part)?;
        let root = doc.root();
        if !root.is(sml::SharedStringTable::QNAME) {
            return Err(Error::Malformed(format!("{part}: root is not sst")));
        }
        let t = Table::read(doc, Some(root), sml::SharedStringItem::QNAME);
        if t.has_ac {
            // indices behind an mc:AlternateContent depend on the reader
            return Err(ooxml_core::Error::UnresolvedSibling(sml::SharedStringTable::QNAME).into());
        }
        Ok(Some(SharedStrings {
            prefix: root.name().prefix().map(str::to_owned),
            entries: t.count,
            reuse: t.reuse,
            refs,
            has_count: root.attr(QName::new("", "count")).is_some(),
            has_unique: root.attr(QName::new("", "uniqueCount")).is_some(),
            part,
        }))
    }

    /// Plans one plain string (already `_xHHHH_`-escaped).
    pub(crate) fn plan(&self, escaped: &str, decoded: &str) -> Result<StringPlan> {
        let key = render::<sml::SharedStringTable>(self.prefix.as_deref(), |p| build_si(p, escaped))?;
        if let Some(&i) = self.reuse.get(&key) {
            return Ok(StringPlan { index: i, append: None, escaped: escaped.into(), decoded: decoded.into() });
        }
        Ok(StringPlan { index: self.entries, append: Some(key), escaped: escaped.into(), decoded: decoded.into() })
    }

    /// Applies a plan and a change in the number of `t="s"` cells. Called
    /// from the apply phase; a failure poisons the workbook.
    pub(crate) fn apply(wb: &mut Workbook, plan: Option<&StringPlan>, refs_delta: i64) -> Result<()> {
        let Some(mut reg) = wb.registries.strings.take() else { return Ok(()) };
        let result = (|| -> Result<()> {
            let doc = wb.pkg.xml_part_mut(&reg.part)?;
            let mut sst = doc.lens_mut::<sml::SharedStringTable>()?;
            if let Some(p) = plan
                && let Some(key) = &p.append
            {
                build_si(&mut sst, &p.escaped)?;
                reg.reuse.insert(key.clone(), reg.entries);
                reg.entries += 1;
                if let Some(cache) = wb.shared_strings.as_mut() {
                    cache.push(p.decoded.clone());
                }
                if reg.has_unique {
                    sst.set_attr(QName::new("", "uniqueCount"), &reg.entries.to_string())?;
                }
            }
            if refs_delta != 0 || plan.is_some_and(|p| p.append.is_some()) {
                reg.refs += refs_delta;
                if reg.has_count {
                    sst.set_attr(QName::new("", "count"), &reg.refs.max(0).to_string())?;
                }
            }
            Ok(())
        })();
        wb.registries.strings = Some(reg);
        result
    }
}
