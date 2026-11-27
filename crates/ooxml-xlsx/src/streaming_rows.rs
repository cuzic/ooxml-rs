use std::{fmt::Write, io::BufRead};

use quick_xml::{
    XmlVersion,
    escape::escape as escape_xml,
    events::{BytesRef, BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};

use crate::{
    CellIndex, CellRange, Error, Result, SML_NS, escape,
    registry::read::declared_font_from_node,
};

const RELATIONSHIP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A streaming row, with its one-based worksheet row number.
pub struct ReadRow {
    pub number: u32,
    pub height: Option<f64>,
    pub style: Option<u32>,
    pub hidden: bool,
    pub outline_level: u32,
    pub collapsed: bool,
    pub cells: Vec<ReadCell>,
}

/// A cell's unstyled payload and references to workbook-level registries.
#[derive(Default)]
pub struct ReadCell {
    pub column: u32,
    pub kind: String,
    pub style: Option<u32>,
    pub value: Option<String>,
    /// Inline rich-text runs with fonts read as written ([`crate::DeclaredFont`]).
    pub rich_text: Vec<(String, Option<crate::DeclaredFont>)>,
    pub formula: Option<String>,
    pub shared_formula: Option<u32>,
    pub formula_range: Option<String>,
}

/// A streamed hyperlink with its worksheet cell range and relationship ID.
pub struct ReadHyperlink {
    pub reference: String,
    pub range: CellRange,
    pub relationship: Option<String>,
    pub location: Option<String>,
    pub display: Option<String>,
    pub tooltip: Option<String>,
}

/// Formatting applied to an inclusive, one-based column interval.
#[derive(Debug, PartialEq)]
pub struct ReadColumn {
    pub first: u32,
    pub last: u32,
    pub width: Option<f64>,
    pub style: Option<u32>,
    pub hidden: bool,
    pub outline_level: u32,
    pub collapsed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Worksheet,
    Data,
    Columns,
    Column,
    Row,
    Cell,
    Formula,
    Value,
    Inline,
    Run,
    RunProperties,
    FontElement,
    Text,
    Hyperlinks,
    Other,
}

/// Reads worksheet XML one row at a time through a buffered source.
pub struct WorksheetRows<R> {
    reader: NsReader<R>,
    buffer: Vec<u8>,
    stack: Vec<Tag>,
    row: Option<ReadRow>,
    cell: Option<ReadCell>,
    last_row: u32,
    root_seen: bool,
    run_start: usize,
    rich_start: usize,
    run_font: Option<crate::DeclaredFont>,
    font_xml: Option<String>,
    links: Option<Vec<ReadHyperlink>>,
    skip_rows: bool,
    columns: Vec<ReadColumn>,
}

impl<R: BufRead> WorksheetRows<R> {
    pub fn new(source: R) -> Self {
        Self {
            reader: NsReader::from_reader(source),
            buffer: Vec::new(),
            stack: Vec::new(),
            row: None,
            cell: None,
            last_row: 0,
            root_seen: false,
            run_start: 0,
            rich_start: 0,
            run_font: None,
            font_xml: None,
            links: None,
            skip_rows: false,
            columns: Vec::new(),
        }
    }

    /// Enables hyperlink capture, optionally scanning without materializing rows.
    pub fn capture_hyperlinks(&mut self, skip_rows: bool) {
        self.links = Some(Vec::new());
        self.skip_rows = skip_rows;
    }

    /// Takes hyperlinks encountered so far.
    pub fn take_hyperlinks(&mut self) -> Vec<ReadHyperlink> {
        self.links.as_mut().map_or_else(Vec::new, std::mem::take)
    }

    /// Takes column formats read before the first row, in worksheet order.
    pub fn take_columns(&mut self) -> Vec<ReadColumn> {
        std::mem::take(&mut self.columns)
    }

    pub fn next_row(&mut self) -> Result<Option<ReadRow>> {
        loop {
            self.buffer.clear();
            let (ns, event) = self
                .reader
                .read_resolved_event_into(&mut self.buffer)
                .map_err(malformed)?;
            match event {
                Event::Start(ref start) | Event::Empty(ref start) => {
                    let parent = self.stack.last().copied();
                    let sml =
                        matches!(ns, ResolveResult::Bound(name) if name.into_inner() == SML_NS);
                    let tag = if matches!(parent, Some(Tag::RunProperties | Tag::FontElement)) {
                        Tag::FontElement
                    } else if sml {
                        match (parent, start.local_name().into_inner()) {
                            (None, "worksheet") => Tag::Worksheet,
                            (Some(Tag::Worksheet), "sheetData") if !self.skip_rows => Tag::Data,
                            (Some(Tag::Worksheet), "cols") if !self.skip_rows => Tag::Columns,
                            (Some(Tag::Columns), "col") => Tag::Column,
                            (Some(Tag::Worksheet), "hyperlinks") if self.links.is_some() => {
                                Tag::Hyperlinks
                            }
                            (Some(Tag::Data), "row") => Tag::Row,
                            (Some(Tag::Row), "c") => Tag::Cell,
                            (Some(Tag::Cell), "f") => Tag::Formula,
                            (Some(Tag::Cell), "v") => Tag::Value,
                            (Some(Tag::Cell), "is") => Tag::Inline,
                            (Some(Tag::Inline), "r") => Tag::Run,
                            (Some(Tag::Run), "rPr") => Tag::RunProperties,
                            (Some(Tag::Inline | Tag::Run), "t") => Tag::Text,
                            _ => Tag::Other,
                        }
                    } else {
                        Tag::Other
                    };
                    if sml
                        && parent == Some(Tag::Hyperlinks)
                        && start.local_name().into_inner() == "hyperlink"
                    {
                        let reference = attr(start, "ref")?
                            .ok_or_else(|| malformed("Missing hyperlink range"))?;
                        let relationship = relationship_id(start, self.reader.resolver())?;
                        let link = ReadHyperlink {
                            range: CellRange::parse(&reference)?,
                            reference,
                            relationship,
                            location: attr(start, "location")?,
                            display: attr(start, "display")?,
                            tooltip: attr(start, "tooltip")?,
                        };
                        self.links
                            .as_mut()
                            .expect("hyperlink capture enabled")
                            .push(link);
                    }
                    if parent.is_none() {
                        if self.root_seen || tag != Tag::Worksheet {
                            return Err(malformed("expected a SpreadsheetML worksheet root"));
                        }
                        self.root_seen = true;
                    }
                    match tag {
                        Tag::Column => self.columns.push(read_column(start)?),
                        Tag::Run => {
                            let cell = self.cell.as_ref().expect("run inside inline cell");
                            self.rich_start = cell.value.as_ref().map_or(0, String::len);
                            self.run_font = None;
                        }
                        Tag::RunProperties => {
                            self.font_xml = Some(format!("<r xmlns=\"{SML_NS}\">"));
                            capture_font_start(
                                &mut self.font_xml,
                                start,
                                matches!(event, Event::Empty(_)),
                            )?;
                        }
                        Tag::FontElement => {
                            if sml {
                                capture_font_start(
                                    &mut self.font_xml,
                                    start,
                                    matches!(event, Event::Empty(_)),
                                )?;
                            } else {
                                self.font_xml = None;
                            }
                        }
                        Tag::Text => {
                            let cell = self.cell.as_ref().expect("text inside inline cell");
                            self.run_start = cell.value.as_ref().map_or(0, String::len);
                        }
                        Tag::Row => {
                            let number = number_attr(start, "r")?.unwrap_or(self.last_row + 1);
                            checked_index(number, 1)?;
                            if number <= self.last_row {
                                return Err(malformed("worksheet row indexes must increase"));
                            }
                            self.last_row = number;
                            self.row = Some(ReadRow {
                                number,
                                height: size_attr(start, "ht")?,
                                style: number_attr(start, "s")?,
                                hidden: bool_attr(start, "hidden")?,
                                outline_level: number_attr(start, "outlineLevel")?.unwrap_or(0),
                                collapsed: bool_attr(start, "collapsed")?,
                                cells: Vec::new(),
                            });
                        }
                        Tag::Cell => {
                            let row = self.row.as_ref().expect("cell inside row");
                            let previous = row.cells.last().map_or(0, |cell| cell.column);
                            let column = if let Some(reference) = attr(start, "r")? {
                                let at = CellIndex::parse(&reference)?;
                                if at.row + 1 != row.number {
                                    return Err(malformed("worksheet cell is outside its row"));
                                }
                                at.col + 1
                            } else {
                                previous + 1
                            };
                            checked_index(row.number, column)?;
                            if column <= previous {
                                return Err(malformed("worksheet column indexes must increase"));
                            }
                            self.cell = Some(ReadCell {
                                column,
                                kind: attr(start, "t")?.unwrap_or_default(),
                                style: number_attr(start, "s")?,
                                ..ReadCell::default()
                            });
                        }
                        Tag::Formula => {
                            let cell = self.cell.as_mut().expect("formula inside cell");
                            cell.formula = Some(String::new());
                            match attr(start, "t")?.as_deref() {
                                Some("shared") => {
                                    cell.shared_formula = number_attr(start, "si")?;
                                    cell.formula_range = attr(start, "ref")?;
                                }
                                Some("array") => cell.formula_range = attr(start, "ref")?,
                                _ => {}
                            }
                        }
                        Tag::Value | Tag::Inline => {
                            self.cell.as_mut().expect("value inside cell").value =
                                Some(String::new());
                        }
                        _ => {}
                    }
                    if matches!(event, Event::Empty(_)) {
                        if let Some(row) = self.finish(tag) {
                            return Ok(Some(row));
                        }
                    } else {
                        self.stack.push(tag);
                    }
                }
                Event::End(end) => {
                    let tag = self
                        .stack
                        .pop()
                        .ok_or_else(|| malformed("unmatched worksheet end tag"))?;
                    if matches!(tag, Tag::RunProperties | Tag::FontElement)
                        && let Some(font) = self.font_xml.as_mut()
                    {
                        write!(font, "</{}>", end.local_name().into_inner()).unwrap();
                    }
                    if let Some(row) = self.finish(tag) {
                        return Ok(Some(row));
                    }
                }
                Event::Text(text) => push_text(
                    &mut self.cell,
                    self.stack.last(),
                    &text.xml_content(XmlVersion::Implicit1_0),
                ),
                Event::CData(text) => push_text(
                    &mut self.cell,
                    self.stack.last(),
                    &text.xml_content(XmlVersion::Implicit1_0),
                ),
                Event::GeneralRef(reference) => {
                    let character = decode_ref(&reference)?;
                    push_text(
                        &mut self.cell,
                        self.stack.last(),
                        character.encode_utf8(&mut [0; 4]),
                    );
                }
                Event::DocType(_) => return Err(malformed("DTD is not allowed in worksheet XML")),
                Event::Eof if self.root_seen && self.stack.is_empty() => return Ok(None),
                Event::Eof => return Err(malformed("truncated worksheet XML")),
                _ => {}
            }
        }
    }

    fn finish(&mut self, tag: Tag) -> Option<ReadRow> {
        match tag {
            Tag::Row => return self.row.take(),
            Tag::Text => {
                let cell = self.cell.as_mut().expect("text inside inline cell");
                if let Some(value) = cell.value.as_mut() {
                    let suffix = &value[self.run_start..];
                    if let std::borrow::Cow::Owned(decoded) = escape::decode(suffix) {
                        value.truncate(self.run_start);
                        value.push_str(&decoded);
                    }
                }
            }
            Tag::RunProperties => {
                self.run_font = self.font_xml.take().and_then(|mut xml| {
                    xml.push_str("</r>");
                    let document = ooxml_core::XmlDocument::parse(xml.as_bytes()).ok()?;
                    declared_font_from_node(document.root().elements().next()?)
                });
            }
            Tag::Run => {
                let cell = self.cell.as_mut().expect("run inside inline cell");
                let text = cell.value.as_deref().unwrap_or_default()[self.rich_start..].to_owned();
                cell.rich_text.push((text, self.run_font.take()));
            }
            Tag::Cell => {
                let mut cell = self.cell.take().expect("started cell");
                if cell.kind == "str" {
                    cell.value = cell.value.map(|value| escape::decode(&value).into_owned());
                }
                self.row.as_mut().expect("cell inside row").cells.push(cell);
            }
            _ => {}
        }
        None
    }
}

fn capture_font_start(
    output: &mut Option<String>,
    start: &BytesStart<'_>,
    empty: bool,
) -> Result<()> {
    let Some(xml) = output.as_mut() else {
        return Ok(());
    };
    write!(xml, "<{}", start.local_name().into_inner()).unwrap();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(malformed)?;
        let name = attribute.key.into_inner();
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        if name.contains(':') {
            *output = None;
            return Ok(());
        }
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(malformed)?;
        write!(xml, " {name}=\"{}\"", escape_xml(value)).unwrap();
    }
    xml.push_str(if empty { "/>" } else { ">" });
    Ok(())
}

fn malformed(error: impl std::fmt::Display) -> Error {
    Error::Malformed(error.to_string())
}

fn checked_index(row: u32, column: u32) -> Result<()> {
    let row = row
        .checked_sub(1)
        .ok_or_else(|| malformed("worksheet row indexes are one-based"))?;
    CellIndex::new(row, column - 1).map(drop)
}

fn attr(start: &BytesStart<'_>, key: &str) -> Result<Option<String>> {
    start
        .try_get_attribute(key)
        .map_err(malformed)?
        .map(|attr| {
            attr.normalized_value(XmlVersion::Implicit1_0)
                .map(|value| value.into_owned())
                .map_err(malformed)
        })
        .transpose()
}

fn read_column(start: &BytesStart<'_>) -> Result<ReadColumn> {
    let first = number_attr(start, "min")?.ok_or_else(|| malformed("Missing column min"))?;
    let last = number_attr(start, "max")?.ok_or_else(|| malformed("Missing column max"))?;
    if first == 0 || first > last || last > crate::MAX_COLS {
        return Err(malformed("Invalid worksheet column interval"));
    }
    Ok(ReadColumn {
        first,
        last,
        width: size_attr(start, "width")?,
        style: number_attr(start, "style")?,
        hidden: bool_attr(start, "hidden")?,
        outline_level: number_attr(start, "outlineLevel")?.unwrap_or(0),
        collapsed: bool_attr(start, "collapsed")?,
    })
}

fn bool_attr(start: &BytesStart<'_>, key: &str) -> Result<bool> {
    Ok(matches!(
        attr(start, key)?.as_deref().map(str::trim),
        Some("1" | "true")
    ))
}

fn size_attr(start: &BytesStart<'_>, key: &str) -> Result<Option<f64>> {
    let size = attr(start, key)?
        .map(|text| text.trim().parse::<f64>().map_err(malformed))
        .transpose()?;
    if size.is_some_and(|value| !value.is_finite() || value < 0.0) {
        return Err(malformed(format!("Invalid worksheet {key} dimension")));
    }
    Ok(size)
}

fn relationship_id(
    start: &BytesStart<'_>,
    resolver: &quick_xml::name::NamespaceResolver,
) -> Result<Option<String>> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(malformed)?;
        let (namespace, local) = resolver.resolve_attribute(attribute.key);
        if local.into_inner() == "id"
            && matches!(namespace, ResolveResult::Bound(name) if name.into_inner() == RELATIONSHIP_NS)
        {
            return attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|value| Some(value.into_owned()))
                .map_err(malformed);
        }
    }
    Ok(None)
}

fn number_attr(start: &BytesStart<'_>, key: &str) -> Result<Option<u32>> {
    attr(start, key)?
        .map(|text| text.parse().map_err(malformed))
        .transpose()
}

fn push_text(cell: &mut Option<ReadCell>, tag: Option<&Tag>, text: &str) {
    if let Some(cell) = cell {
        let target = match tag {
            Some(Tag::Formula) => &mut cell.formula,
            Some(Tag::Value | Tag::Text) => &mut cell.value,
            _ => return,
        };
        target.get_or_insert_with(String::new).push_str(text);
    }
}

fn decode_ref(reference: &BytesRef<'_>) -> Result<char> {
    Ok(match reference.resolve_char_ref().map_err(malformed)? {
        Some(character) => character,
        None => match &**reference {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            other => return Err(malformed(format!("undeclared entity &{other};"))),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn reads_sparse_prefixed_rows_across_buffer_boundaries() {
        let xml = format!(
            r#"<s:worksheet xmlns:s="{SML_NS}"><s:sheetData>
          <s:row r="2" ht="21.5" s="5" hidden="true" outlineLevel="3" collapsed="1"><s:c r="B2" t="inlineStr"><s:is><s:r><s:rPr><s:rFont val="Calibri"/><s:b/></s:rPr><s:t>a&amp;</s:t></s:r>
          <s:r><s:t>_x0001_&#13;</s:t></s:r></s:is></s:c>
          <s:c r="D2" s="3" t="b"><s:f>2&lt;1</s:f><s:v>0</s:v></s:c></s:row>
          <s:row r="4"/><s:row><s:c t="s"><s:v>7</s:v></s:c></s:row>
          </s:sheetData></s:worksheet>"#
        );
        let mut reader = WorksheetRows::new(BufReader::with_capacity(5, xml.as_bytes()));
        let row = reader.next_row().unwrap().unwrap();
        assert_eq!(row.number, 2);
        assert_eq!(row.height, Some(21.5));
        assert_eq!(row.style, Some(5));
        assert_eq!(row.outline_level, 3);
        assert!(row.hidden && row.collapsed);
        assert_eq!(row.cells[0].column, 2);
        assert_eq!(row.cells[0].value.as_deref(), Some("a&\u{1}\r"));
        assert_eq!(row.cells[0].rich_text[0].0, "a&");
        assert!(row.cells[0].rich_text[0].1.as_ref().unwrap().font.bold);
        assert_eq!(row.cells[0].rich_text[1], ("\u{1}\r".into(), None));
        // The run font is read as written: a name but no size was declared.
        assert_eq!(row.cells[0].rich_text[0].1.as_ref().unwrap().fields, crate::DECLARED_FONT_NAME);
        assert_eq!(row.cells[1].column, 4);
        assert_eq!(row.cells[1].style, Some(3));
        assert_eq!(row.cells[1].formula.as_deref(), Some("2<1"));
        assert_eq!(row.cells[1].value.as_deref(), Some("0"));
        assert!(reader.next_row().unwrap().unwrap().cells.is_empty());
        let row = reader.next_row().unwrap().unwrap();
        assert_eq!(row.number, 5);
        assert_eq!(row.cells[0].kind, "s");
        assert_eq!(row.cells[0].value.as_deref(), Some("7"));
        assert!(reader.next_row().unwrap().is_none());
    }

    #[test]
    fn captures_hyperlinks_without_materializing_rows() {
        let xml = format!(
            r#"<worksheet xmlns="{SML_NS}" xmlns:custom="{RELATIONSHIP_NS}">
            <sheetData><row r="1"><c r="A1"><v>42</v></c></row></sheetData>
            <hyperlinks><hyperlink ref="$B$2:C3" custom:id="rId4" tooltip="a&amp;b"/>
            <hyperlink ref="D4" location="Other!A1" display="Go"/></hyperlinks></worksheet>"#
        );
        let mut reader = WorksheetRows::new(xml.as_bytes());
        reader.capture_hyperlinks(true);
        assert!(reader.next_row().unwrap().is_none());
        let links = reader.take_hyperlinks();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].range, CellRange::parse("B2:C3").unwrap());
        assert_eq!(links[0].relationship.as_deref(), Some("rId4"));
        assert_eq!(links[0].tooltip.as_deref(), Some("a&b"));
        assert_eq!(links[1].location.as_deref(), Some("Other!A1"));
        assert_eq!(links[1].display.as_deref(), Some("Go"));
    }

    #[test]
    fn reads_column_intervals_before_yielding_first_row() {
        let xml = format!(
            r#"<worksheet xmlns="{SML_NS}"><cols>
            <col min="2" max="4" width="15.5" style="3" hidden="1" outlineLevel="2" collapsed="true"/>
            <col min="6" max="6"/></cols>
            <sheetData><row r="1"><c r="F1"><v>9</v></c></row></sheetData></worksheet>"#
        );
        let mut reader = WorksheetRows::new(xml.as_bytes());
        assert_eq!(reader.next_row().unwrap().unwrap().number, 1);
        let columns = reader.take_columns();
        assert_eq!(
            columns,
            [
                ReadColumn {
                    first: 2,
                    last: 4,
                    width: Some(15.5),
                    style: Some(3),
                    hidden: true,
                    outline_level: 2,
                    collapsed: true
                },
                ReadColumn {
                    first: 6,
                    last: 6,
                    width: None,
                    style: None,
                    hidden: false,
                    outline_level: 0,
                    collapsed: false
                },
            ]
        );
        assert!(reader.take_columns().is_empty());
        let invalid = xml.replace("max=\"6\"", "max=\"16385\"");
        assert!(WorksheetRows::new(invalid.as_bytes()).next_row().is_err());
    }

    #[test]
    fn rejects_truncation_mismatched_coordinates_and_dtd() {
        for xml in [
            format!(r#"<worksheet xmlns="{SML_NS}"><sheetData><row r="1">"#),
            format!(r#"<worksheet xmlns="{SML_NS}"><sheetData><row r="2"><c r="A1"/></row>"#),
            "<!DOCTYPE worksheet><worksheet/>".to_owned(),
        ] {
            assert!(WorksheetRows::new(xml.as_bytes()).next_row().is_err());
        }
    }
}
