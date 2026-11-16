//! `docProps/app.xml` upkeep (ADR-0009 §11): `HeadingPairs` and
//! `TitlesOfParts` follow a structural operation, but only for a recognized
//! shape, and structure is never invented.
//!
//! - **Recognized shape:** `HeadingPairs` is a `vt:vector` of (`vt:lpstr`
//!   label, `vt:i4` count) variant pairs; `TitlesOfParts` is a `vt:lpstr`
//!   vector whose length is the sum of the counts; the **first** block lists
//!   the worksheets that are not `veryHidden`, in workbook order.
//! - Other blocks are matched by content: a block equal to the chart sheets
//!   (or the macro sheets) that are not `veryHidden` is that kind's block;
//!   any other block (e.g. "Named Ranges") keeps its titles, except that a
//!   rename rewrites sheet-prefixed titles (`'Old'!Print_Area`) and a delete
//!   drops titles prefixed with the deleted sheet.
//! - Labels are never changed; blocks are never created or removed. If the
//!   result would need a block that does not exist (a kind with sheets to
//!   list and no block, where the old list was empty), or if the shape is not
//!   recognized, app.xml is left untouched for the operation.

use ooxml_core::{ElementSpec, XmlDocument};
use ooxml_schema::props;

use super::names::{PrefixEdit, mentions_prefix, rewrite_location};
use crate::census::SheetKind;
use crate::error::Result;

/// The relationship type of `docProps/app.xml`.
pub(crate) const REL_EXTENDED: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";

/// One sheet as app.xml sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) name: String,
    pub(crate) kind: SheetKind,
    pub(crate) very_hidden: bool,
}

fn listed_names(sheets: &[Listed], kind: SheetKind) -> Vec<String> {
    sheets.iter().filter(|s| s.kind == kind && !s.very_hidden).map(|s| s.name.clone()).collect()
}

/// `(label, titles)` blocks of a recognized app.xml.
fn blocks(doc: &XmlDocument) -> Option<Vec<(String, Vec<String>)>> {
    let p = props::PropertiesView(doc.lens::<props::Properties>().ok()?);
    let hp = p.heading_pairs()?.vt_vector()?;
    let tp = p.titles_of_parts()?.vt_vector()?;
    // every child of both vectors must be what the shape says
    if hp.raw().elements().any(|e| !e.is(props::Variant::QNAME)) || tp.raw().elements().any(|e| !e.is(props::VTLPSTR::QNAME)) {
        return None;
    }
    let variants: Vec<_> = hp.variant_iter().collect();
    if !variants.len().is_multiple_of(2) {
        return None;
    }
    let titles: Vec<String> = tp.vtlpstr_iter().map(|t| crate::escape::decode(&t.0.text()).into_owned()).collect();
    let mut out = Vec::new();
    let mut at = 0usize;
    for pair in variants.chunks(2) {
        let label = pair[0].vtlpstr()?.0.text().into_owned();
        let count: usize = pair[1].vt_int32()?.0.text().trim().parse().ok()?;
        if pair[0].raw().elements().count() != 1 || pair[1].raw().elements().count() != 1 {
            return None;
        }
        let end = at.checked_add(count)?;
        out.push((label, titles.get(at..end)?.to_vec()));
        at = end;
    }
    (at == titles.len()).then_some(out)
}

/// The new app.xml content for an operation, or `None` to leave it
/// untouched. `before`/`after`: the sheets before and after the operation.
pub(crate) fn update(doc: &XmlDocument, before: &[Listed], after: &[Listed], edit: Option<&PrefixEdit<'_>>, deleted: Option<&str>) -> Result<Option<XmlDocument>> {
    let Some(old) = blocks(doc) else { return Ok(None) };
    let Some(first) = old.first() else { return Ok(None) };
    if first.1 != listed_names(before, SheetKind::Worksheet) {
        return Ok(None);
    }
    let kinds = [SheetKind::Worksheet, SheetKind::Chartsheet, SheetKind::MacroSheet, SheetKind::DialogSheet];
    let mut new_blocks = old.clone();
    let mut covered = Vec::new();
    for (bi, (_, titles)) in old.iter().enumerate() {
        let kind = if bi == 0 {
            Some(SheetKind::Worksheet)
        } else {
            kinds[1..].iter().copied().find(|k| !covered.contains(k) && {
                let l = listed_names(before, *k);
                !l.is_empty() && l == *titles
            })
        };
        match kind {
            Some(k) => {
                covered.push(k);
                new_blocks[bi].1 = listed_names(after, k);
            }
            None => {
                // another block (Named Ranges): follow a rename, drop a deleted sheet's names
                let mut t = Vec::new();
                for title in titles {
                    if let Some(d) = deleted
                        && mentions_prefix(title, d)
                    {
                        continue;
                    }
                    match edit.and_then(|e| rewrite_location(title, e)) {
                        Some(n) => t.push(n),
                        None => t.push(title.clone()),
                    }
                }
                new_blocks[bi].1 = t;
            }
        }
    }
    // a kind that now has sheets to list but no block: structure would be invented
    for k in kinds {
        if !covered.contains(&k) && !listed_names(after, k).is_empty() && listed_names(before, k).is_empty() {
            return Ok(None);
        }
    }
    if new_blocks == old {
        return Ok(None);
    }
    let mut out = doc.clone();
    write(&mut out, &new_blocks)?;
    Ok(Some(out))
}

fn write(doc: &mut XmlDocument, blocks: &[(String, Vec<String>)]) -> Result<()> {
    let titles: Vec<&String> = blocks.iter().flat_map(|(_, t)| t).collect();
    let mut p = props::PropertiesViewMut(doc.lens_mut::<props::Properties>()?);
    {
        let mut hp = p.heading_pairs_mut().expect("recognized");
        let mut v = hp.vt_vector_mut().expect("recognized");
        for (bi, (_, t)) in blocks.iter().enumerate() {
            let n = 2 * bi + 1;
            let var = v.0.nth_child_mut::<props::Variant>(n).expect("recognized");
            let mut var = props::VariantViewMut(var);
            let mut i4 = var.vt_int32_mut().expect("recognized");
            if i4.as_view().0.text().trim() != t.len().to_string() {
                i4.set_text(&t.len().to_string())?;
            }
        }
    }
    let mut tp = p.titles_of_parts_mut().expect("recognized");
    let mut v = tp.vt_vector_mut().expect("recognized");
    let old_len = v.as_view().vtlpstr_iter().count();
    for (i, t) in titles.iter().enumerate() {
        let enc = crate::escape::encode(t);
        if i < old_len {
            let mut l = props::VTLPSTRViewMut(v.0.nth_child_mut::<props::VTLPSTR>(i).expect("counted"));
            if crate::escape::decode(&l.as_view().0.text()) != t.as_str() {
                l.set_text(&enc)?;
            }
        } else {
            v.insert_vtlpstr()?.set_text(&enc)?;
        }
    }
    for i in (titles.len()..old_len).rev() {
        let idx = v.0.node().indexed_elements().filter(|(_, e)| e.is(props::VTLPSTR::QNAME)).nth(i).map(|(k, _)| k).expect("counted");
        v.0.remove_at(idx);
    }
    if v.as_view().size().ok().flatten() != Some(titles.len() as u32) {
        v.set_size(titles.len() as u32)?;
    }
    let mut hp = p.heading_pairs_mut().expect("recognized");
    let mut hv = hp.vt_vector_mut().expect("recognized");
    let pairs = 2 * blocks.len() as u32;
    if hv.as_view().size().ok().flatten() != Some(pairs) {
        hv.set_size(pairs)?;
    }
    Ok(())
}
