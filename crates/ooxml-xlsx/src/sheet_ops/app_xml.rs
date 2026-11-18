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
//! - Labels are never changed and blocks are never created. A block that an
//!   operation empties is removed with its pair, as Excel writes it (no
//!   count-0 pairs; PR #55 review m1). If the result would need a block that
//!   does not exist (a kind with sheets to list and no block, where the old
//!   list was empty), or if the shape is not recognized, app.xml is left
//!   untouched for the operation.
//! - Titles are `xsd:string` (plain text, not ST_Xstring): read and written
//!   as they are (review m2).
//! - Recognition includes each vector's `size` (equal to its child count)
//!   and `baseType` (`variant` / `lpstr`) (review m3).

use ooxml_core::{ElementSpec, QName, XmlDocument};
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

// The root schema marker belongs to the pml feature. Check its QName
// here so XLSX can use the typed children without enabling that feature.
fn property_index<E: ElementSpec>(doc: &XmlDocument) -> Option<usize> {
    let root = doc.root();
    if !root.is(QName::new("http://schemas.openxmlformats.org/officeDocument/2006/extended-properties", "Properties")) {
        return None;
    }
    root.indexed_elements().find_map(|(i, e)| e.is(E::QNAME).then_some(i))
}

/// `(label, titles)` blocks of a recognized app.xml.
fn blocks(doc: &XmlDocument) -> Option<Vec<(String, Vec<String>)>> {
    let hp = props::HeadingPairsView(doc.lens_at(&[property_index::<props::HeadingPairs>(doc)?]).ok()?);
    let tp = props::TitlesOfPartsView(doc.lens_at(&[property_index::<props::TitlesOfParts>(doc)?]).ok()?);
    let hp = hp.vt_vector()?;
    let tp = tp.vt_vector()?;
    // every child of both vectors must be what the shape says, and each
    // vector's size and baseType must agree with them
    if hp.raw().elements().any(|e| !e.is(props::Variant::QNAME)) || tp.raw().elements().any(|e| !e.is(props::VTLPSTR::QNAME)) {
        return None;
    }
    let base = |v: &ooxml_core::RawNode| v.attr(ooxml_core::QName::new("", "baseType")).map(str::trim).map(str::to_owned);
    if base(hp.raw()).as_deref() != Some("variant") || base(tp.raw()).as_deref() != Some("lpstr") {
        return None;
    }
    if hp.size().ok().flatten() != Some(hp.raw().elements().count() as u32) || tp.size().ok().flatten() != Some(tp.raw().elements().count() as u32) {
        return None;
    }
    let variants: Vec<_> = hp.variant_iter().collect();
    if !variants.len().is_multiple_of(2) {
        return None;
    }
    let titles: Vec<String> = tp.vtlpstr_iter().map(|t| t.0.text().into_owned()).collect();
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
    // a block the operation emptied goes, with its pair (review m1)
    let gone: Vec<usize> = (0..old.len()).filter(|&i| !old[i].1.is_empty() && new_blocks[i].1.is_empty()).collect();
    let mut out = doc.clone();
    write(&mut out, &new_blocks, &gone)?;
    Ok(Some(out))
}

/// Writes `blocks` (one per existing pair); the pairs of `gone` (emptied
/// blocks) are removed.
fn write(doc: &mut XmlDocument, blocks: &[(String, Vec<String>)], gone: &[usize]) -> Result<()> {
    let titles: Vec<&String> = blocks.iter().flat_map(|(_, t)| t).collect();
    let hp_path = [property_index::<props::HeadingPairs>(doc).expect("recognized")];
    let tp_path = [property_index::<props::TitlesOfParts>(doc).expect("recognized")];
    {
        let mut hp = props::HeadingPairsViewMut(doc.lens_mut_at(&hp_path)?);
        let mut v = hp.vt_vector_mut().expect("recognized");
        for (bi, (_, t)) in blocks.iter().enumerate() {
            if gone.contains(&bi) {
                continue;
            }
            let n = 2 * bi + 1;
            let var = v.0.nth_child_mut::<props::Variant>(n).expect("recognized");
            let mut var = props::VariantViewMut(var);
            let mut i4 = var.vt_int32_mut().expect("recognized");
            if i4.as_view().0.text().trim() != t.len().to_string() {
                i4.set_text(&t.len().to_string())?;
            }
        }
        for &bi in gone.iter().rev() {
            for k in [2 * bi + 1, 2 * bi] {
                let idx = v.0.node().indexed_elements().filter(|(_, e)| e.is(props::Variant::QNAME)).nth(k).map(|(i, _)| i).expect("recognized");
                v.0.remove_at(idx);
            }
        }
    }
    let mut tp = props::TitlesOfPartsViewMut(doc.lens_mut_at(&tp_path)?);
    let mut v = tp.vt_vector_mut().expect("recognized");
    let old_len = v.as_view().vtlpstr_iter().count();
    for (i, t) in titles.iter().enumerate() {
        if i < old_len {
            let mut l = props::VTLPSTRViewMut(v.0.nth_child_mut::<props::VTLPSTR>(i).expect("counted"));
            if l.as_view().0.text() != t.as_str() {
                l.set_text(t)?;
            }
        } else {
            v.insert_vtlpstr()?.set_text(t)?;
        }
    }
    for i in (titles.len()..old_len).rev() {
        let idx = v.0.node().indexed_elements().filter(|(_, e)| e.is(props::VTLPSTR::QNAME)).nth(i).map(|(k, _)| k).expect("counted");
        v.0.remove_at(idx);
    }
    if v.as_view().size().ok().flatten() != Some(titles.len() as u32) {
        v.set_size(titles.len() as u32)?;
    }
    let mut hp = props::HeadingPairsViewMut(doc.lens_mut_at(&hp_path)?);
    let mut hv = hp.vt_vector_mut().expect("recognized");
    let pairs = 2 * (blocks.len() - gone.len()) as u32;
    if hv.as_view().size().ok().flatten() != Some(pairs) {
        hv.set_size(pairs)?;
    }
    Ok(())
}
