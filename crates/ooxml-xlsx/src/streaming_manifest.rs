use ooxml_core::{
    EnumValue, XmlDocument,
    opc::{PACKAGE_RELS_PART, REL_OFFICE_DOCUMENT, Relationships, rels_part_name},
};
use ooxml_schema::sml;

use crate::{
    Error, Result, RichTextRun, escape,
    registry::read::{DeclaredFont, border_from_node, declared_font_from_node, fill_from_node, run_font_from_node},
    workbook::{REL_SHARED_STRINGS, REL_WORKSHEET, is_sheet_relationship},
};

const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// Parts and settings needed to stream a workbook.
pub struct WorkbookManifest {
    pub date1904: bool,
    pub strings: Option<String>,
    pub styles: Option<String>,
    pub sheets: Vec<WorksheetPart>,
    pub defined_names: Vec<String>,
}

/// A worksheet in workbook order.
pub struct WorksheetPart {
    pub id: u32,
    pub name: String,
    pub state: &'static str,
    pub path: String,
}

/// Resolves workbook metadata through an OPC part reader.
pub fn read_workbook_manifest(
    mut read: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<WorkbookManifest> {
    let root = Relationships::parse(None, &read(PACKAGE_RELS_PART)?)?;
    let path = root
        .by_type(REL_OFFICE_DOCUMENT)
        .find_map(|rel| root.resolve(rel))
        .ok_or(Error::NoWorkbook)?;
    let document = XmlDocument::parse(&read(&path)?)?;
    let workbook = sml::WorkbookView(document.lens::<sml::Workbook>()?);
    let rels = Relationships::parse(Some(&path), &read(&rels_part_name(Some(&path)))?)?;
    let mut sheets = Vec::new();
    let sheet_list = workbook.sheets();
    let mut listed = sheet_list.iter().flat_map(|list| list.sheet_iter()).peekable();
    if listed.peek().is_none() && rels.iter().any(|rel| is_sheet_relationship(&rel.rel_type)) {
        return Err(Error::Malformed("Workbook has sheet relationships but no listed sheets".into()));
    }
    for sheet in listed {
        let name = sheet
            .name()
            .ok_or_else(|| Error::Malformed("Missing workbook sheet name".into()))?;
        let relationship = sheet.id().and_then(|id| rels.by_id(id)).ok_or_else(|| {
            Error::Malformed(format!("Missing worksheet relationship for {name:?}"))
        })?;
        if relationship.rel_type != REL_WORKSHEET {
            continue;
        }
        let path = rels.resolve(relationship).ok_or_else(|| {
            Error::Malformed(format!("External worksheet relationship for {name:?}"))
        })?;
        let id = sheet
            .sheet_id()
            .map_err(ooxml_core::Error::from)?
            .ok_or_else(|| Error::Malformed(format!("Missing sheet ID for {name:?}")))?;
        let state = match sheet.state() {
            EnumValue::Known(sml::SheetStateValues::Hidden) => "hidden",
            EnumValue::Known(sml::SheetStateValues::VeryHidden) => "veryHidden",
            _ => "visible",
        };
        sheets.push(WorksheetPart {
            id,
            name: escape::decode(name).into_owned(),
            state,
            path,
        });
    }
    let date1904 = workbook
        .workbook_properties()
        .map(|properties| properties.date1904())
        .transpose()
        .map_err(ooxml_core::Error::from)?
        .unwrap_or(false);
    let defined_names = workbook.defined_names().iter()
        .flat_map(|names| names.defined_name_iter())
        .filter_map(|item| item.name().map(str::to_owned))
        .collect();
    let part = |kind| rels.by_type(kind).find_map(|rel| rels.resolve(rel));
    Ok(WorkbookManifest {
        date1904,
        strings: part(REL_SHARED_STRINGS),
        styles: part(REL_STYLES),
        sheets,
        defined_names,
    })
}

/// A plain shared string or the text runs of a rich shared string.
#[derive(Debug, PartialEq, Eq)]
pub enum SharedText {
    Plain(String),
    Rich(Vec<RichTextRun>),
}

/// Reads the shared-string registry in index order.
pub fn read_shared_text(bytes: &[u8]) -> Result<Vec<SharedText>> {
    read_shared_text_records(bytes, false, |text, _| text)
}

/// Reads shared strings with one flag byte per rich text run.
/// The bits are [`crate::DeclaredFont::fields`]: bit 0 marks an explicit font
/// name, bit 1 an explicit size, and the higher bits `outline`, `shadow`,
/// `condense` and `extend`. Run fonts are read as written.
pub fn read_shared_text_with_font_fields(bytes: &[u8]) -> Result<Vec<(SharedText, Vec<u8>)>> {
    read_shared_text_records(bytes, true, |text, fields| (text, fields))
}

fn read_shared_text_records<T>(
    bytes: &[u8],
    track_font_fields: bool,
    project: impl Fn(SharedText, Vec<u8>) -> T,
) -> Result<Vec<T>> {
    let document = XmlDocument::parse(bytes)?;
    let strings = sml::SharedStringTableView(document.lens::<sml::SharedStringTable>()?);
    let text = |view: Option<sml::TextView<'_>>| {
        view.map_or_else(String::new, |view| {
            escape::decode(&view.text()).into_owned()
        })
    };
    Ok(strings
        .shared_string_item_iter()
        .map(|item| {
            let mut font_fields = Vec::new();
            let runs = item
                .run_iter()
                .map(|run| {
                    let properties = run.run_properties();
                    let font = if track_font_fields {
                        // Read as written, so `outline` / `shadow` keep the font.
                        let declared = properties.and_then(|properties| declared_font_from_node(properties.0.raw()));
                        font_fields.push(declared.as_ref().map_or(0, |declared| declared.fields));
                        declared.map(|declared| declared.font)
                    } else {
                        properties.and_then(|properties| run_font_from_node(properties.0.raw()))
                    };
                    (text(run.text()), font)
                })
                .collect::<Vec<_>>();
            if runs.is_empty() {
                project(SharedText::Plain(text(item.text())), font_fields)
            } else {
                project(SharedText::Rich(runs), font_fields)
            }
        })
        .collect())
}

/// Number format attached to a streamed cell style.
#[derive(Debug, PartialEq, Eq)]
pub enum StyleNumberFormat {
    Builtin(u32),
    Custom(String),
}

/// The cached font and number-format references for a streamed style.
pub struct StreamingStyle {
    pub number_format: StyleNumberFormat,
    pub font_id: Option<u32>,
    pub fill_id: Option<u32>,
    pub border_id: Option<u32>,
    pub alignment: Vec<(String, String)>,
    pub protection: Option<crate::Protection>,
}

/// Style records and their shared font table.
pub struct StreamingStyles {
    pub styles: Vec<StreamingStyle>,
    /// Fonts as written; absent `name` / `sz` are flagged, not defaulted.
    pub fonts: Vec<Option<DeclaredFont>>,
    pub fills: Vec<Option<crate::Fill>>,
    pub borders: Vec<Option<crate::Border>>,
}

/// Reads fonts and number formats referenced by cell styles.
pub fn read_streaming_styles(bytes: &[u8]) -> Result<StreamingStyles> {
    let document = XmlDocument::parse(bytes)?;
    let styles = sml::StylesheetView(document.lens::<sml::Stylesheet>()?);
    let mut custom = std::collections::HashMap::new();
    if let Some(formats) = styles.numbering_formats() {
        for format in formats.numbering_format_iter() {
            if let (Some(id), Some(code)) = (
                format.number_format_id().map_err(ooxml_core::Error::from)?,
                format.format_code(),
            ) {
                custom.insert(id, code);
            }
        }
    }
    let fonts = styles
        .fonts()
        .map(|table| {
            table
                .font_iter()
                .map(|font| declared_font_from_node(font.raw()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let fills = styles
        .fills()
        .map(|table| {
            table
                .fill_iter()
                .map(|fill| fill_from_node(fill.raw()).ok())
                .collect()
        })
        .unwrap_or_default();
    let borders = styles
        .borders()
        .map(|table| {
            table
                .border_iter()
                .map(|border| border_from_node(border.raw()).ok())
                .collect()
        })
        .unwrap_or_default();
    let mut result = StreamingStyles {
        styles: Vec::new(),
        fonts,
        fills,
        borders,
    };
    if let Some(formats) = styles.cell_formats() {
        for style in formats.cell_format_iter() {
            let id = style
                .number_format_id()
                .map_err(ooxml_core::Error::from)?
                .unwrap_or(0);
            let font_id = style.font_id().map_err(ooxml_core::Error::from)?;
            let number_format = custom
                .get(&id)
                .map_or(StyleNumberFormat::Builtin(id), |code| {
                    StyleNumberFormat::Custom((*code).to_owned())
                });
            result.styles.push(StreamingStyle {
                number_format,
                font_id,
                fill_id: style.fill_id().map_err(ooxml_core::Error::from)?,
                border_id: style.border_id().map_err(ooxml_core::Error::from)?,
                alignment: style
                    .alignment()
                    .map(|view| crate::setters::project_alignment(view.raw()))
                    .unwrap_or_default(),
                protection: style
                    .protection()
                    .map(|view| crate::setters::project_protection(view.raw())),
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DECLARED_FONT_CONDENSE, DECLARED_FONT_EXTEND, DECLARED_FONT_NAME, DECLARED_FONT_OUTLINE, DECLARED_FONT_SHADOW, DECLARED_FONT_SIZE};

    #[test]
    fn reads_shared_text_runs_and_custom_formats() {
        let strings = read_shared_text(br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>_x0041_&amp;</t></si><si><r><t xml:space="preserve"> a </t></r><r><t>b</t></r><rPh sb="0" eb="1"><t>ignored</t></rPh></si></sst>"#).unwrap();
        assert_eq!(
            strings,
            [
                SharedText::Plain("A&".into()),
                SharedText::Rich(vec![(" a ".into(), None), ("b".into(), None)])
            ]
        );
        let formats = read_streaming_styles(br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts><fonts><font><name val="Calibri"/><sz val="12"/><b/></font></fonts><cellXfs><xf/><xf numFmtId="14" fontId="0"/><xf numFmtId="164"/></cellXfs></styleSheet>"#).unwrap();
        assert!(formats.fonts[0].as_ref().unwrap().font.bold);
        assert_eq!(formats.styles[1].font_id, Some(0));
        assert_eq!(
            formats
                .styles
                .into_iter()
                .map(|style| style.number_format)
                .collect::<Vec<_>>(),
            [
                StyleNumberFormat::Builtin(0),
                StyleNumberFormat::Builtin(14),
                StyleNumberFormat::Custom("yyyy-mm-dd".into())
            ]
        );
        assert!(read_shared_text(b"<!DOCTYPE sst><sst/>").is_err());
    }

    #[test]
    fn distinguishes_absent_and_explicit_default_font_fields() {
        let strings = read_shared_text_with_font_fields(br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si>
            <r><rPr><b/></rPr><t>A</t></r>
            <r><rPr><sz val="11"/></rPr><t>B</t></r>
            <r><rPr><rFont val="Calibri"/><sz val="11"/></rPr><t>C</t></r>
            <r><t>D</t></r>
        </si></sst>"#).unwrap();
        assert_eq!(strings[0].1, [0, 2, 3, 0]);
    }

    #[test]
    fn keeps_run_fonts_with_presence_only_elements_when_tracking_fields() {
        let xml = br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si>
            <r><rPr><b/><outline/></rPr><t>A</t></r>
            <r><rPr><shadow/><condense/></rPr><t>B</t></r>
        </si></sst>"#;
        let tracked = read_shared_text_with_font_fields(xml).unwrap();
        let (SharedText::Rich(runs), fields) = &tracked[0] else { panic!("expected rich text") };
        assert!(runs[0].1.as_ref().unwrap().bold);
        assert!(runs[1].1.is_some());
        assert_eq!(*fields, [DECLARED_FONT_OUTLINE, DECLARED_FONT_SHADOW | DECLARED_FONT_CONDENSE]);
        // Without field tracking, fonts a `Font` cannot hold are still dropped.
        let plain = read_shared_text(xml).unwrap();
        let [SharedText::Rich(plain)] = plain.as_slice() else { panic!("expected rich text") };
        assert_eq!(plain[0].1, None);
    }

    #[test]
    fn reads_cell_fonts_without_name_or_size_and_flags_declared_fields() {
        let styles = read_streaming_styles(br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts>
            <font><b/><color rgb="FFFF0000"/></font>
            <font><i/><sz val="14"/></font>
            <font><name val="Arial"/></font>
            <font><name val="Calibri"/><sz val="11"/></font>
            <font/>
            <font><outline/></font>
            <font><shadow val="0"/><condense val="0"/><extend/></font>
            <font><futureFontProperty/></font>
        </fonts></styleSheet>"#).unwrap();
        let fields = styles.fonts.iter().map(|font| font.as_ref().map(|font| font.fields)).collect::<Vec<_>>();
        assert_eq!(fields, [Some(0), Some(DECLARED_FONT_SIZE), Some(DECLARED_FONT_NAME),
            Some(DECLARED_FONT_NAME | DECLARED_FONT_SIZE), Some(0), Some(DECLARED_FONT_OUTLINE),
            Some(DECLARED_FONT_SHADOW | DECLARED_FONT_CONDENSE | DECLARED_FONT_EXTEND), None]);
        let bold = &styles.fonts[0].as_ref().unwrap().font;
        assert!(bold.bold);
        assert_eq!(bold.color, Some(crate::Color::Rgb(0xFFFF_0000)));
        assert_eq!(styles.fonts[1].as_ref().unwrap().font.size.get(), 14.0);
        assert_eq!(styles.fonts[2].as_ref().unwrap().font.name, "Arial");
    }

    #[test]
    fn preserves_supported_run_fonts_and_falls_back_to_text_for_unknown_fonts() {
        let strings = read_shared_text(br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si>
            <r><rPr><rFont val="Calibri"/><sz val="12"/><b/><color rgb="FF123456"/></rPr><t>Styled</t></r>
            <r><rPr><futureFontProperty/></rPr><t>Fallback</t></r>
        </si></sst>"#).unwrap();
        let [SharedText::Rich(runs)] = strings.as_slice() else {
            panic!("expected rich shared string")
        };
        let font = runs[0].1.as_ref().unwrap();
        assert_eq!(font.name, "Calibri");
        assert!(font.bold);
        assert_eq!(runs[1], ("Fallback".into(), None));
    }

    #[test]
    fn resolves_nonstandard_workbook_locations_and_sheet_order() {
        let manifest = read_workbook_manifest(|path| Ok(match path {
            "_rels/.rels" => r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="book" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="parts/book.xml"/></Relationships>"#,
            "parts/book.xml" => r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="true"/><sheets><sheet name="A_x0042_" sheetId="9" state="hidden" r:id="first"/><sheet name="Visible" sheetId="3" r:id="second"/></sheets></workbook>"#,
            "parts/_rels/book.xml.rels" => r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="second" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="visible.xml"/><Relationship Id="first" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="../sheets/hidden.xml"/><Relationship Id="strings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="/registry/text.xml"/></Relationships>"#,
            _ => panic!("unexpected read of {path}"),
        }.as_bytes().to_vec())).unwrap();
        assert!(manifest.date1904);
        assert_eq!(manifest.strings.as_deref(), Some("registry/text.xml"));
        assert_eq!(
            manifest
                .sheets
                .iter()
                .map(|sheet| (
                    sheet.id,
                    sheet.name.as_str(),
                    sheet.state,
                    sheet.path.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                (9, "AB", "hidden", "sheets/hidden.xml"),
                (3, "Visible", "visible", "parts/visible.xml")
            ]
        );
    }
}
