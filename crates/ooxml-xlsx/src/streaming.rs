//! Serialization of independent rows for newly authored workbook parts.

use std::{borrow::Cow, collections::HashMap, fmt::Write, sync::Arc};

pub use crate::streaming_manifest::{
    SharedText, StreamingStyle, StreamingStyles, StyleNumberFormat, WorkbookManifest,
    WorksheetPart, read_shared_text, read_shared_text_with_font_fields, read_streaming_styles, read_workbook_manifest,
};
pub use crate::streaming_rows::{ReadCell, ReadColumn, ReadHyperlink, ReadRow, WorksheetRows};
use quick_xml::escape::partial_escape;

use ooxml_core::EnumType;
use ooxml_schema::sml::CellValues;

use crate::{
    CellIndex, CellValue, Error, Result, dynamic::formula_cache, edit::validate_formula_text,
};

/// Expands a shared formula using the names declared in workbook metadata.
pub fn translate_shared_formula(text: &str, from: CellIndex, to: CellIndex, names: &[String]) -> Option<String> {
    use crate::formula::precedents::fold;
    let names: std::collections::HashSet<_> = names.iter().map(|name| fold(name)).collect();
    let dr = i64::from(to.row) - i64::from(from.row);
    let dc = i64::from(to.col) - i64::from(from.col);
    crate::formula::translate::translate(text, dr, dc, &|name| names.contains(&fold(name))).ok()
}

/// One value or formula, with a style index from the writer's workbook.
pub struct RowCell {
    /// Optional rich-text value.
    pub rich_text: Option<Vec<crate::RichTextRun>>,
    /// Cell value or the cached formula result.
    pub value: CellValue,
    /// Optional formula in file form.
    pub formula: Option<RowFormula>,
    /// Optional index into the destination workbook styles.
    pub style: Option<u32>,
}

/// Formula stored on a streaming cell.
pub enum RowFormula {
    Ordinary(String),
    Array {
        text: String,
        range: crate::CellRange,
    },
    SharedMaster {
        text: String,
        range: crate::CellRange,
        index: u32,
    },
    SharedMember {
        index: u32,
    },
}

impl From<&str> for RowFormula {
    fn from(text: &str) -> Self {
        Self::Ordinary(text.to_owned())
    }
}

/// Layout attributes to write independently of the row's cell values.
#[derive(Clone, Copy, Default)]
pub struct RowLayout {
    pub style: Option<u32>,
    pub height: Option<f64>,
    pub hidden: bool,
    pub outline_level: u8,
    pub collapsed: bool,
}

impl From<CellValue> for RowCell {
    fn from(value: CellValue) -> Self {
        Self {
            value,
            formula: None,
            style: None,
            rich_text: None,
        }
    }
}

/// Encodes one SpreadsheetML row without retaining worksheet state.
/// Row and column indexes are zero-based. Columns must be in ascending order.
pub fn encode_row(row: u32, cells: impl IntoIterator<Item = (u32, CellValue)>) -> Result<String> {
    encode_cells(
        row,
        cells
            .into_iter()
            .map(|(column, value)| (column, value.into())),
    )
}

/// Encodes one row with optional formulas and style indexes.
pub fn encode_cells(row: u32, cells: impl IntoIterator<Item = (u32, RowCell)>) -> Result<String> {
    encode_cells_with_layout(row, cells, RowLayout::default())
}

/// Workbook-wide interning table for strings in streaming rows.
#[derive(Default)]
pub struct SharedTextWriter {
    entries: Vec<(bool, Arc<str>)>,
    indexes: [HashMap<Arc<str>, u32>; 2],
}

impl SharedTextWriter {
    /// Number of distinct committed strings.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Serializes a batch of items for inclusion in the shared string part.
    pub fn items_xml(&self, start: usize, count: usize) -> String {
        let mut xml = String::new();
        for (rich, text) in self.entries.iter().skip(start).take(count) {
            if *rich {
                xml.push_str(text);
                continue;
            }
            write!(
                xml,
                "<si><t xml:space=\"preserve\">{}</t></si>",
                escape_xml(text)
            )
            .unwrap();
        }
        xml
    }

    /// Writes a row, interning its strings. Rejected rows leave the table unchanged.
    pub fn encode_row(
        &mut self,
        row: u32,
        cells: impl IntoIterator<Item = (u32, RowCell)>,
        layout: RowLayout,
    ) -> Result<String> {
        let checkpoint = self.entries.len();
        let result = encode_row_inner(row, cells, layout, Some(self));
        if result.is_err() {
            for (rich, text) in self.entries.drain(checkpoint..) {
                self.indexes[usize::from(rich)].remove(&text);
            }
        }
        result
    }

    fn intern(&mut self, text: String, rich: bool) -> Result<u32> {
        let indexes = &mut self.indexes[usize::from(rich)];
        if let Some(&index) = indexes.get(text.as_str()) {
            return Ok(index);
        }
        let index = self
            .entries
            .len()
            .try_into()
            .map_err(|_| Error::Malformed("too many shared strings".into()))?;
        let text = Arc::<str>::from(text);
        indexes.insert(text.clone(), index);
        self.entries.push((rich, text));
        Ok(index)
    }
}

/// Encodes one row with optional formatting attributes.
pub fn encode_cells_with_layout(
    row: u32,
    cells: impl IntoIterator<Item = (u32, RowCell)>,
    layout: RowLayout,
) -> Result<String> {
    encode_row_inner(row, cells, layout, None)
}

fn encode_row_inner(
    row: u32,
    cells: impl IntoIterator<Item = (u32, RowCell)>,
    layout: RowLayout,
    mut strings: Option<&mut SharedTextWriter>,
) -> Result<String> {
    CellIndex::new(row, 0)?;
    if layout.outline_level > 7 {
        return Err(Error::InvalidOutlineLevel(layout.outline_level));
    }
    let mut xml = format!("<row r=\"{}\"", row + 1);
    if let Some(height) = layout.height {
        if !(0.0..=409.0).contains(&height) {
            return Err(Error::InvalidSetting {
                what: "row height".into(),
                reason: format!("{height} is outside 0–409"),
            });
        }
        write!(xml, " ht=\"{height}\" customHeight=\"1\"").unwrap();
    }
    if let Some(style) = layout.style {
        write!(xml, " s=\"{style}\" customFormat=\"1\"").unwrap();
    }
    if layout.hidden {
        xml.push_str(" hidden=\"1\"");
    }
    if layout.outline_level != 0 {
        write!(xml, " outlineLevel=\"{}\"", layout.outline_level).unwrap();
    }
    if layout.collapsed {
        xml.push_str(" collapsed=\"1\"");
    }
    xml.push('>');
    let mut previous = None;
    for (
        column,
        RowCell {
            value,
            formula,
            style,
            rich_text,
        },
    ) in cells
    {
        let at = CellIndex::new(row, column)?;
        if previous.is_some_and(|previous| previous >= column) {
            return Err(Error::InvalidValue {
                cell: at.to_string(),
                message: "streaming row columns must be strictly increasing".into(),
            });
        }
        previous = Some(column);
        let text = match &formula {
            Some(RowFormula::Array { range, .. }) if range.first != at => {
                return Err(Error::InvalidValue {
                    cell: at.to_string(),
                    message: "array formula must start at its anchor cell".into(),
                });
            }
            Some(RowFormula::SharedMaster { range, .. }) if range.first != at => {
                return Err(Error::InvalidValue {
                    cell: at.to_string(),
                    message: "shared formula must start at its anchor cell".into(),
                });
            }
            Some(
                RowFormula::Ordinary(text)
                | RowFormula::Array { text, .. }
                | RowFormula::SharedMaster { text, .. },
            ) => Some(text.strip_prefix('=').unwrap_or(text)),
            _ => None,
        };
        if let Some(text) = text {
            validate_formula_text(text)?;
        }
        let rich_inline = rich_text.is_some() && strings.is_none();
        let (kind, content) = if let Some(rich_text) = rich_text {
            if formula.is_some() || value != CellValue::Empty {
                return Err(Error::InvalidValue {
                    cell: at.to_string(),
                    message: "rich text must be a cell value".into(),
                });
            }
            let runs = rich_text
                .iter()
                .map(|(text, font)| (font.as_ref(), text.as_str()))
                .collect::<Vec<_>>();
            let (runs, _) = crate::setters::prepare_rich_text(at, &runs)?;
            if let Some(strings) = strings.as_deref_mut() {
                let xml = crate::registry::strings::render_rich_item(runs)?;
                (
                    CellValues::SharedString,
                    strings.intern(xml, true)?.to_string(),
                )
            } else {
                (
                    CellValues::InlineString,
                    crate::registry::strings::render_inline_rich(&runs)?,
                )
            }
        } else {
            let Some((kind, content)) = formula_cache(at, value)?.or_else(|| {
                (formula.is_some() || style.is_some()).then(|| (CellValues::Number, String::new()))
            }) else {
                continue;
            };
            if formula.is_some() || kind != CellValues::String {
                (kind, content)
            } else if let Some(strings) = strings.as_deref_mut() {
                (
                    CellValues::SharedString,
                    strings.intern(content, false)?.to_string(),
                )
            } else {
                (CellValues::InlineString, content)
            }
        };
        write!(
            xml,
            "<c r=\"{at}\" t=\"{}\"",
            CellValues::VALUES[kind.index()]
        )
        .unwrap();
        if let Some(style) = style {
            write!(xml, " s=\"{style}\"").unwrap();
        }
        xml.push('>');
        match &formula {
            Some(RowFormula::Ordinary(_)) => {
                write!(
                    xml,
                    "<f>{}</f>",
                    escape_xml(text.expect("ordinary formula text"))
                )
                .unwrap();
            }
            Some(RowFormula::Array { range, .. }) => {
                write!(
                    xml,
                    "<f t=\"array\" ref=\"{range}\">{}</f>",
                    escape_xml(text.expect("array formula text"))
                )
                .unwrap();
            }
            Some(RowFormula::SharedMaster { range, index, .. }) => {
                write!(
                    xml,
                    "<f t=\"shared\" ref=\"{range}\" si=\"{index}\">{}</f>",
                    escape_xml(text.expect("shared formula text"))
                )
                .unwrap();
            }
            Some(RowFormula::SharedMember { index }) => {
                write!(xml, "<f t=\"shared\" si=\"{index}\"/>").unwrap();
            }
            None => {}
        }
        if rich_inline {
            xml.push_str(&content);
        } else if kind == CellValues::InlineString {
            write!(
                xml,
                "<is><t xml:space=\"preserve\">{}</t></is>",
                escape_xml(&content)
            )
            .unwrap();
        } else if !content.is_empty() || kind == CellValues::String {
            write!(xml, "<v>{}</v>", escape_xml(&content)).unwrap();
        }
        xml.push_str("</c>");
    }
    xml.push_str("</row>");
    Ok(xml)
}

fn escape_xml(text: &str) -> Cow<'_, str> {
    let escaped = partial_escape(text);
    if escaped.contains('\r') {
        escaped.replace('\r', "&#13;").into()
    } else {
        escaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_row_escapes_text_and_preserves_scalar_types() {
        let xml = encode_row(
            1,
            [
                (0, CellValue::String(" <&\r_x0041_\u{1} ".into())),
                (1, CellValue::Empty),
                (2, CellValue::Bool(false)),
                (26, CellValue::Number(42.0)),
            ],
        )
        .unwrap();
        assert_eq!(
            xml,
            concat!(
                "<row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">",
                " &lt;&amp;&#13;_x005F_x0041__x0001_ </t></is></c>",
                "<c r=\"C2\" t=\"b\"><v>0</v></c><c r=\"AA2\" t=\"n\"><v>42</v></c></row>",
            )
        );
    }

    #[test]
    fn formula_escapes_text_and_preserves_cached_boolean() {
        let xml = encode_cells(
            0,
            [(
                0,
                RowCell {
                    value: CellValue::Bool(false),
                    formula: Some("=1<0".into()),
                    style: Some(1),
                    rich_text: None,
                },
            )],
        )
        .unwrap();
        assert!(xml.contains("t=\"b\" s=\"1\"><f>1&lt;0</f><v>0</v>"));
        assert!(
            encode_cells(
                0,
                [(
                    0,
                    RowCell {
                        value: CellValue::Empty,
                        formula: Some("SUM((".into()),
                        style: None,
                        rich_text: None,
                    }
                )]
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_layout_on_rows_without_cells() {
        let layout = RowLayout {
            height: Some(18.5),
            style: Some(3),
            hidden: true,
            outline_level: 2,
            collapsed: true,
        };
        assert_eq!(
            encode_cells_with_layout(7, [], layout).unwrap(),
            r#"<row r="8" ht="18.5" customHeight="1" s="3" customFormat="1" hidden="1" outlineLevel="2" collapsed="1"></row>"#
        );
        assert!(
            encode_cells_with_layout(
                0,
                [],
                RowLayout {
                    height: Some(410.0),
                    ..layout
                }
            )
            .is_err()
        );
    }

    #[test]
    fn shares_plain_text_transactionally_and_preserves_formula_results() {
        let mut writer = SharedTextWriter::default();
        assert!(
            writer
                .encode_row(
                    0,
                    [
                        (0, CellValue::String("unused".into()).into()),
                        (1, CellValue::Number(f64::NAN).into()),
                    ],
                    RowLayout::default()
                )
                .is_err()
        );
        assert!(writer.is_empty());
        let plain = CellValue::String(" &_x0041_ ".into());
        let xml = writer
            .encode_row(
                0,
                [
                    (0, plain.clone().into()),
                    (1, plain.clone().into()),
                    (
                        2,
                        RowCell {
                            value: plain,
                            formula: Some("A1".into()),
                            style: None,
                            rich_text: None,
                        },
                    ),
                ],
                RowLayout::default(),
            )
            .unwrap();
        assert_eq!(writer.len(), 1);
        assert!(xml.contains(r#"<c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>0</v></c>"#));
        assert!(xml.contains(r#"<c r="C1" t="str"><f>A1</f><v> &amp;_x005F_x0041_ </v></c>"#));
        assert_eq!(
            writer.items_xml(0, 256),
            r#"<si><t xml:space="preserve"> &amp;_x005F_x0041_ </t></si>"#
        );
    }

    #[test]
    fn registers_styles_without_materializing_cells() {
        let mut book = crate::Workbook::new().unwrap();
        let font = crate::Font {
            bold: true,
            ..crate::Font::default()
        };
        let style = crate::Format::with_font(font.clone());
        let index = book.register_streaming_style(style.clone()).unwrap();
        assert!(index > 0);
        assert_eq!(book.register_streaming_style(style).unwrap(), index);
        assert!(book.worksheet().unwrap().cell(0, 0).unwrap().is_none());
        let mut reopened = crate::Workbook::from_bytes(book.to_bytes().unwrap()).unwrap();
        assert_eq!(reopened.font_of_style(index).unwrap().unwrap(), font);
    }

    #[test]
    fn writes_array_formula_references_only_from_the_anchor() {
        let formula = || RowCell {
            formula: Some(RowFormula::Array {
                text: "A3:B4*2".into(),
                range: crate::CellRange::parse("A1:B2").unwrap(),
            }),
            ..CellValue::Number(2.0).into()
        };
        let xml = encode_cells(0, [(0, formula())]).unwrap();
        assert!(xml.contains(r#"<f t="array" ref="A1:B2">A3:B4*2</f><v>2</v>"#));
        assert!(encode_cells(0, [(1, formula())]).is_err());
    }

    #[test]
    fn rejects_invalid_rows_before_returning_xml() {
        assert!(encode_row(crate::MAX_ROWS, []).is_err());
        assert!(encode_row(0, [(0, CellValue::Empty), (0, CellValue::Empty)]).is_err());
        assert!(encode_row(0, [(0, CellValue::Number(f64::NAN))]).is_err());
        assert!(encode_row(0, [(0, CellValue::String("😺".repeat(16_384)))]).is_err());
    }
}
