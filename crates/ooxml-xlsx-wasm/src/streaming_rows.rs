use std::io::{self, BufReader, Read};

use js_sys::{Function, Uint8Array};
use ooxml_xlsx::streaming::WorksheetRows;
use wasm_bindgen::prelude::*;

use crate::js_error;

struct Source(Function);

impl Read for Source {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let requested = JsValue::from(buffer.len() as u32);
        let result = self
            .0
            .call1(&JsValue::UNDEFINED, &requested)
            .map_err(source_error)?;
        copy_bytes(result, buffer)
    }
}

pub(crate) fn source_error(error: JsValue) -> io::Error {
    let message = error
        .dyn_ref::<js_sys::Error>()
        .map(|error| error.message().into())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "XLSX source callback failed".into());
    io::Error::other(message)
}

pub(crate) fn copy_bytes(result: JsValue, buffer: &mut [u8]) -> io::Result<usize> {
    let bytes = result
        .dyn_into::<Uint8Array>()
        .map_err(|_| io::Error::other("XLSX source must return a Uint8Array"))?;
    let length = bytes.length() as usize;
    if length > buffer.len() {
        return Err(io::Error::other(
            "XLSX source exceeded the requested length",
        ));
    }
    bytes.copy_to(&mut buffer[..length]);
    Ok(length)
}

/// Pulls worksheet rows through a synchronous callback that returns up to the
/// requested number of XML bytes and returns an empty array at EOF.
#[wasm_bindgen]
pub struct XlsxRowStream {
    rows: WorksheetRows<BufReader<Box<dyn Read>>>,
}

#[wasm_bindgen]
impl XlsxRowStream {
    #[wasm_bindgen(constructor)]
    pub fn new(read: Function) -> Self {
        Self::from_source(Source(read))
    }

    /// Returns column-format intervals encountered before the first row.
    pub fn take_columns(&mut self) -> Vec<JsValue> {
        self.rows
            .take_columns()
            .into_iter()
            .flat_map(|column| {
                [
                    column.first.into(),
                    column.last.into(),
                    column.width.map_or(JsValue::UNDEFINED, JsValue::from),
                    column.style.map_or(JsValue::UNDEFINED, JsValue::from),
                    column.hidden.into(),
                    column.outline_level.into(),
                    column.collapsed.into(),
                ]
            })
            .collect()
    }

    /// Enables hyperlink collection for this sheet.
    pub fn capture_hyperlinks(&mut self, skip_rows: bool) {
        self.rows.capture_hyperlinks(skip_rows);
    }

    /// Returns captured hyperlink records with one-based range coordinates.
    pub fn take_hyperlinks(&mut self) -> Vec<JsValue> {
        self.rows
            .take_hyperlinks()
            .into_iter()
            .flat_map(|link| {
                [
                    link.reference.into(),
                    (link.range.first.row + 1).into(),
                    (link.range.first.col + 1).into(),
                    (link.range.last.row + 1).into(),
                    (link.range.last.col + 1).into(),
                    link.relationship.map_or(JsValue::UNDEFINED, JsValue::from),
                    link.location.map_or(JsValue::UNDEFINED, JsValue::from),
                    link.display.map_or(JsValue::UNDEFINED, JsValue::from),
                    link.tooltip.map_or(JsValue::UNDEFINED, JsValue::from),
                ]
            })
            .collect()
    }

    /// Returns an empty array at EOF. Otherwise, the row number is followed by
    /// tuples of column, type, style, text, formula and shared-formula index.
    /// When requested, layout fields follow the row number before the cell tuples.
    pub fn next_row(&mut self, include_layout: Option<bool>) -> Result<Vec<JsValue>, JsError> {
        let Some(row) = self.rows.next_row().map_err(js_error)? else {
            return Ok(Vec::new());
        };
        let include_layout = include_layout.unwrap_or(false);
        let mut output =
            Vec::with_capacity(if include_layout { 6 } else { 1 } + row.cells.len() * 6);
        output.push(row.number.into());
        if include_layout {
            output.extend([
                row.height.map_or(JsValue::UNDEFINED, JsValue::from),
                row.style.map_or(JsValue::UNDEFINED, JsValue::from),
                row.hidden.into(),
                row.outline_level.into(),
                row.collapsed.into(),
            ]);
        }
        for cell in row.cells {
            output.extend([
                cell.column.into(),
                cell.kind.into(),
                cell.style.map_or(JsValue::UNDEFINED, JsValue::from),
                if cell.rich_text.is_empty() {
                    cell.value.map_or(JsValue::UNDEFINED, JsValue::from)
                } else {
                    crate::encode_declared_rich_text_runs(cell.rich_text)
                        .into_iter()
                        .map(JsValue::from)
                        .collect::<js_sys::Array>()
                        .into()
                },
                match (cell.formula, cell.formula_range) {
                    (Some(text), Some(reference)) => {
                        [JsValue::from(text), JsValue::from(reference)]
                            .into_iter()
                            .collect::<js_sys::Array>()
                            .into()
                    }
                    (text, _) => text.map_or(JsValue::UNDEFINED, JsValue::from),
                },
                cell.shared_formula
                    .map_or(JsValue::UNDEFINED, JsValue::from),
            ]);
        }
        Ok(output)
    }
}

impl XlsxRowStream {
    pub(crate) fn from_source(source: impl Read + 'static) -> Self {
        Self {
            rows: WorksheetRows::new(BufReader::with_capacity(64 * 1024, Box::new(source))),
        }
    }
}
