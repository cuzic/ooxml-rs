//! Table style definitions (ADR-0016 §9).

use ooxml_core::name::{MC_ALTERNATE_CONTENT, MC_NS, QName};
use ooxml_core::{
    ChildOf, ElementSpec, LensMut, Package, PartNameRequest, RawNode, RegistryPlan, Target, XmlDocument,
};
use ooxml_schema::dml;
use sha2::{Digest, Sha256};

use super::{ColorSpec, LineSpec, RunFill, ThemeFont};
use crate::builtin_styles::builtin;
use crate::error::{Error, Result};
use crate::names::A_NS;

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
const CT: &str = "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml";
const PART: &str = "ppt/tableStyles.xml";
const DEFAULT_STYLE: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

/// A table style GUID in uppercase, braced ST_Guid form.
pub type StyleId = String;

/// A new named table style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleDef {
    /// The display name.
    pub name: String,
    /// Formatting for the table and its regions.
    pub parts: TableStyleParts,
}

/// Optional regions of a table style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleParts {
    /// Whole-table formatting.
    pub whole_table: Option<TablePartStyle>,
    /// First horizontal band.
    pub band1_horizontal: Option<TablePartStyle>,
    /// Second horizontal band.
    pub band2_horizontal: Option<TablePartStyle>,
    /// First vertical band.
    pub band1_vertical: Option<TablePartStyle>,
    /// Second vertical band.
    pub band2_vertical: Option<TablePartStyle>,
    /// First row.
    pub first_row: Option<TablePartStyle>,
    /// Last row.
    pub last_row: Option<TablePartStyle>,
    /// First column.
    pub first_column: Option<TablePartStyle>,
    /// Last column.
    pub last_column: Option<TablePartStyle>,
}

/// Text and cell formatting for one table region.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TablePartStyle {
    /// Text formatting.
    pub text: Option<TableStyleText>,
    /// Cell formatting.
    pub cell: Option<TableStyleCell>,
}

/// Optional text overrides in a table region.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleText {
    /// Bold override.
    pub bold: Option<bool>,
    /// Italic override.
    pub italic: Option<bool>,
    /// Theme font collection.
    pub font: Option<ThemeFont>,
    /// Text colour.
    pub color: Option<ColorSpec>,
}

/// Fill and borders in a table region.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleCell {
    /// Solid colour or explicit no fill. Omit to inherit.
    pub fill: Option<RunFill>,
    /// The four outside and two inside borders.
    pub borders: TableStyleBorders,
}

/// Optional line formatting for the six table borders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleBorders {
    /// Left border.
    pub left: Option<LineSpec>,
    /// Right border.
    pub right: Option<LineSpec>,
    /// Top border.
    pub top: Option<LineSpec>,
    /// Bottom border.
    pub bottom: Option<LineSpec>,
    /// Borders between rows.
    pub inside_horizontal: Option<LineSpec>,
    /// Borders between columns.
    pub inside_vertical: Option<LineSpec>,
}

pub(crate) fn style_ids(doc: &XmlDocument) -> Result<Vec<String>> {
    Ok(doc
        .lens::<dml::TableStyleList>()?
        .children::<dml::TableStyleEntry>()
        .filter_map(|node| {
            node.attr(dml::TableStyleEntry::ATTR_STYLE_ID)
                .map(str::to_owned)
        })
        .collect())
}

pub(crate) fn existing_part(pkg: &Package, main: &str) -> Result<Option<String>> {
    let rels = pkg.relationships(main)?;
    Ok(rels
        .by_type(REL)
        .filter_map(|rel| rels.resolve(rel))
        .find(|part| pkg.contains(part)))
}

/// Reserve IDs in every choice and fallback, descending only through MC wrappers.
fn branch_style_ids<'a>(parent: &'a RawNode, ids: &mut Vec<&'a str>) {
    for wrapper in parent.elements().filter(|node| node.is(MC_ALTERNATE_CONTENT)) {
        for branch in wrapper.elements().filter(|node| {
            node.is(QName::new(MC_NS, "Choice")) || node.is(QName::new(MC_NS, "Fallback"))
        }) {
            ids.extend(
                branch.elements()
                    .filter(|node| node.is(dml::TableStyleEntry::QNAME))
                    .filter_map(|node| node.attr(dml::TableStyleEntry::ATTR_STYLE_ID)),
            );
            branch_style_ids(branch, ids);
        }
    }
}

fn style_id(name: &str, ids: &[String], branch_ids: &[&str]) -> StyleId {
    let mut seed = Sha256::new();
    for value in std::iter::once(name).chain(ids.iter().map(String::as_str)) {
        seed.update((value.len() as u64).to_be_bytes());
        seed.update(value.as_bytes());
    }
    let candidates = (0u64..).map(|attempt| {
        let mut hash = seed.clone();
        hash.update(attempt.to_be_bytes());
        let digest = hash.finalize();
        format!(
            "{{{:08X}-{:04X}-{:04X}-{:04X}-{:04X}{:08X}}}",
            u32::from_be_bytes(digest[0..4].try_into().unwrap()),
            u16::from_be_bytes(digest[4..6].try_into().unwrap()),
            u16::from_be_bytes(digest[6..8].try_into().unwrap()) & 0x0fff | 0x8000,
            u16::from_be_bytes(digest[8..10].try_into().unwrap()) & 0x3fff | 0x8000,
            u16::from_be_bytes(digest[10..12].try_into().unwrap()),
            u32::from_be_bytes(digest[12..16].try_into().unwrap()),
        )
    });
    available_style_id(
        ids.iter().map(String::as_str).chain(branch_ids.iter().copied()),
        candidates,
    )
    .expect("unbounded candidate sequence")
}

fn available_style_id<'a>(
    ids: impl Iterator<Item = &'a str> + Clone,
    mut candidates: impl Iterator<Item = StyleId>,
) -> Option<StyleId> {
    candidates.find(|id| {
        builtin(id).is_none() && !ids.clone().any(|old| old.trim().eq_ignore_ascii_case(id))
    })
}

impl crate::Presentation {
    /// Appends a definition and returns the ID accepted by [`crate::TableMut::set_style`].
    /// Creates the style part and its registry entries if needed.
    pub fn add_table_style(&mut self, definition: TableStyleDef) -> Result<StyleId> {
        let existing = existing_part(&self.pkg, &self.main)?;
        let mut doc = if let Some(part) = &existing {
            self.pkg.xml_part(part)?.clone()
        } else {
            let rels = self.pkg.relationships(&self.main)?;
            if let Some(rel) = rels.by_type(REL).next() {
                // Exact allocation reports the dangling target as in use even
                // when a producer chose a nonstandard part name.
                if let Some(target) = rels.resolve(rel) {
                    self.pkg.free_part_name(PartNameRequest::Exact(target))?;
                }
                return Err(Error::UnsupportedContent(
                    "tableStyles relationship has no internal target part".into(),
                ));
            }
            XmlDocument::parse(
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<a:tblStyleLst xmlns:a=\"{A_NS}\" def=\"{DEFAULT_STYLE}\"/>"
                )
                .as_bytes(),
            )?
        };
        let mut branch_ids = Vec::new();
        branch_style_ids(doc.root(), &mut branch_ids);
        let id = style_id(&definition.name, &style_ids(&doc)?, &branch_ids);
        let mut list = doc.lens_mut::<dml::TableStyleList>()?;
        let mut entry = dml::TableStyleEntryViewMut(list.insert::<dml::TableStyleEntry>()?);
        entry.set_style_id(&id)?;
        entry.set_style_name(&definition.name)?;
        definition.parts.write(&mut entry.0)?;

        let mut plan = RegistryPlan::new(&self.pkg);
        if let Some(part) = existing {
            plan.stage_part(&self.pkg, &part, doc)?;
        } else {
            let part = plan.add_part(&self.pkg, PartNameRequest::Exact(PART.into()), CT, doc)?;
            plan.add_relationship(&self.pkg, Some(&self.main), REL, Target::Part(part))?;
        }
        self.pkg.apply(plan)?;
        Ok(id)
    }
}

impl TableStyleParts {
    fn write(&self, entry: &mut LensMut<'_, dml::TableStyleEntry>) -> Result<()> {
        macro_rules! regions {
            ($($field:ident: $spec:ident),* $(,)?) => {
                $(if let Some(region) = &self.$field {
                    region.write(&mut entry.insert::<dml::$spec>()?)?;
                })*
            };
        }
        regions! {
            whole_table: WholeTable,
            band1_horizontal: Band1Horizontal,
            band2_horizontal: Band2Horizontal,
            band1_vertical: Band1Vertical,
            band2_vertical: Band2Vertical,
            last_column: LastColumn,
            first_column: FirstColumn,
            last_row: LastRow,
            first_row: FirstRow,
        }
        Ok(())
    }
}

impl TablePartStyle {
    fn write<E: ElementSpec>(&self, region: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::TableCellTextStyle: ChildOf<E>,
        dml::TableCellStyle: ChildOf<E>,
    {
        if let Some(text) = &self.text {
            let mut node = region.insert::<dml::TableCellTextStyle>()?;
            for (attr, flag) in [
                (dml::TableCellTextStyle::ATTR_B, text.bold),
                (dml::TableCellTextStyle::ATTR_I, text.italic),
            ] {
                if let Some(flag) = flag {
                    node.set_attr(attr, if flag { "on" } else { "off" })?;
                }
            }
            if let Some(font) = text.font {
                dml::FontReferenceViewMut(node.insert::<dml::FontReference>()?).set_index(
                    match font {
                        ThemeFont::Major => dml::FontCollectionIndexValues::Major,
                        ThemeFont::Minor => dml::FontCollectionIndexValues::Minor,
                    },
                )?;
            }
            if let Some(color) = &text.color {
                color.validate()?;
                color.write(&mut node)?;
            }
        }
        if let Some(cell) = &self.cell {
            let mut node = region.insert::<dml::TableCellStyle>()?;
            cell.borders.write(&mut node)?;
            if let Some(fill) = &cell.fill {
                let mut fill_node = node.insert::<dml::FillProperties>()?;
                match fill {
                    RunFill::NoFill => {
                        fill_node.insert::<dml::NoFill>()?;
                    }
                    RunFill::Solid(color) => {
                        color.validate()?;
                        color.write(&mut fill_node.insert::<dml::SolidFill>()?)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl TableStyleBorders {
    fn write(&self, cell: &mut LensMut<'_, dml::TableCellStyle>) -> Result<()> {
        let mut borders = None;
        macro_rules! lines {
            ($($field:ident: $spec:ident),* $(,)?) => {
                $(if let Some(line) = &self.$field {
                    line.validate()?;
                    if borders.is_none() {
                        borders = Some(cell.insert::<dml::TableCellBorders>()?);
                    }
                    let mut edge = borders.as_mut().unwrap().insert::<dml::$spec>()?;
                    line.write(&mut edge.insert::<dml::Outline>()?)?;
                })*
            };
        }
        lines! {
            left: LeftBorder,
            right: RightBorder,
            top: TopBorder,
            bottom: BottomBorder,
            inside_horizontal: InsideHorizontalBorder,
            inside_vertical: InsideVerticalBorder,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_uuid_v8_with_collision_retry() {
        let shape = regex::Regex::new(
            r"^\{[0-9A-F]{8}-[0-9A-F]{4}-8[0-9A-F]{3}-[89AB][0-9A-F]{3}-[0-9A-F]{12}\}$",
        )
        .unwrap();
        let existing = "{01234567-89AB-8CDE-8F00-0123456789AB}";
        let ids = vec![existing.to_ascii_lowercase()];
        let id = style_id("Accent", &ids, &[]);
        assert!(shape.is_match(&id), "{id}");
        assert_eq!(id, style_id("Accent", &ids, &[]));
        assert_eq!(builtin(&id), None);
        assert!(!existing.eq_ignore_ascii_case(&id));

        let collisions = [DEFAULT_STYLE.to_owned(), existing.to_owned(), id.clone()];
        assert_eq!(
            available_style_id(ids.iter().map(String::as_str), collisions.into_iter()),
            Some(id)
        );
    }
}
