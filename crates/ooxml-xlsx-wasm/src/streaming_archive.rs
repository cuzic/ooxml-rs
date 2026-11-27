use std::io::{self, Read, Seek, SeekFrom};

use crc32fast::Hasher;
use flate2::read::DeflateDecoder;
use js_sys::Function;
use wasm_bindgen::prelude::*;
use zip::{CompressionMethod, ZipArchive};

use crate::{
    streaming_rows::{XlsxRowStream, copy_bytes, source_error},
    zip_io_error,
};

#[derive(Clone)]
struct ArchiveSource {
    read_at: Function,
    length: u64,
    position: u64,
}

impl Read for ArchiveSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.length.saturating_sub(self.position);
        let limit = (buffer.len() as u64).min(remaining).min(64 * 1024) as usize;
        if limit == 0 {
            return Ok(0);
        }
        let bytes = self
            .read_at
            .call2(
                &JsValue::UNDEFINED,
                &(self.position as f64).into(),
                &(limit as u32).into(),
            )
            .map_err(source_error)?;
        let length = copy_bytes(bytes, &mut buffer[..limit])?;
        self.position += length as u64;
        Ok(length)
    }
}

impl Seek for ArchiveSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.length.checked_add_signed(offset),
        };
        self.position = position.ok_or_else(|| io::Error::other("Invalid ZIP source offset"))?;
        Ok(self.position)
    }
}

struct CheckedEntry<R> {
    inner: R,
    remaining: u64,
    checksum: Hasher,
    expected_checksum: u32,
}

impl<R: Read> Read for CheckedEntry<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = self.inner.read(buffer)?;
        self.remaining = self
            .remaining
            .checked_sub(length as u64)
            .ok_or_else(|| io::Error::other("ZIP entry exceeds its declared size"))?;
        self.checksum.update(&buffer[..length]);
        if length == 0
            && (self.remaining != 0 || self.checksum.clone().finalize() != self.expected_checksum)
        {
            return Err(io::Error::other("ZIP entry size or checksum mismatch"));
        }
        Ok(length)
    }
}

/// Reads the ZIP directory through a bounded, seekable host callback.
#[wasm_bindgen]
pub struct XlsxZipReader {
    archive: ZipArchive<ArchiveSource>,
    source: ArchiveSource,
}

#[wasm_bindgen]
impl XlsxZipReader {
    #[wasm_bindgen(constructor)]
    pub fn new(length: f64, read_at: Function) -> Result<Self, JsError> {
        if !(0.0..=9_007_199_254_740_991.0).contains(&length) || length.fract() != 0.0 {
            return Err(JsError::new("Expected a safe integer ZIP source length"));
        }
        let source = ArchiveSource {
            read_at,
            length: length as u64,
            position: 0,
        };
        let archive = ZipArchive::new(source.clone()).map_err(zip_io_error)?;
        Ok(Self { archive, source })
    }

    pub fn entry_names(&self) -> Result<Vec<String>, JsError> {
        self.archive
            .file_names()
            .map(|name| name.map(|name| name.into_owned()).map_err(zip_io_error))
            .collect()
    }

    /// Reads a metadata part with an explicit limit on decompressed bytes.
    pub fn read_metadata(&mut self, name: &str, limit: u32) -> Result<Vec<u8>, JsError> {
        self.read_metadata_bytes(name, limit).map_err(zip_io_error)
    }

    /// Returns the workbook's date system and part paths, followed by worksheet records.
    /// With `include_names`, adds the name array before worksheet records.
    pub fn workbook_manifest(&mut self, include_names: Option<bool>) -> Result<Vec<JsValue>, JsError> {
        let manifest = ooxml_xlsx::streaming::read_workbook_manifest(|name| {
            self.read_metadata_bytes(name, u32::MAX).map_err(|error| {
                ooxml_xlsx::Error::Malformed(format!("Cannot read metadata part {name}: {error}"))
            })
        })
        .map_err(crate::js_error)?;
        let mut output = Vec::with_capacity(3 + manifest.sheets.len() * 4);
        output.extend([
            manifest.date1904.into(),
            manifest.strings.map_or(JsValue::UNDEFINED, JsValue::from),
            manifest.styles.map_or(JsValue::UNDEFINED, JsValue::from),
        ]);
        if include_names == Some(true) {
            output.push(manifest.defined_names.into_iter().map(JsValue::from).collect::<js_sys::Array>().into());
        }
        for sheet in manifest.sheets {
            output.extend([
                sheet.id.into(),
                sheet.name.into(),
                sheet.state.into(),
                sheet.path.into(),
            ]);
        }
        Ok(output)
    }

    /// Returns shared strings in index order, using arrays for rich text runs.
    pub fn shared_text(&mut self, name: &str, explicit_font_fields: Option<bool>) -> Result<Vec<JsValue>, JsError> {
        use ooxml_xlsx::streaming::{SharedText, read_shared_text, read_shared_text_with_font_fields};
        let bytes = self.read_metadata(name, u32::MAX)?;
        let rich_fields = |runs| {
            crate::encode_rich_text_runs(runs).into_iter().map(JsValue::from).collect::<js_sys::Array>()
        };
        if explicit_font_fields == Some(true) {
            let strings = read_shared_text_with_font_fields(&bytes).map_err(crate::js_error)?;
            return Ok(strings.into_iter().map(|(text, fields)| match text {
                SharedText::Plain(text) => text.into(),
                SharedText::Rich(runs) => js_sys::Array::of2(
                    &rich_fields(runs),
                    &js_sys::Uint8Array::from(fields.as_slice()),
                ).into(),
            }).collect());
        }
        let strings = read_shared_text(&bytes).map_err(crate::js_error)?;
        Ok(strings.into_iter().map(|text| match text {
            SharedText::Plain(text) => text.into(),
            SharedText::Rich(runs) => rich_fields(runs).into(),
        }).collect())
    }

    /// Returns shared style groups followed by records for cell, row and column formats.
    pub fn cell_styles(&mut self, name: &str) -> Result<Vec<JsValue>, JsError> {
        use ooxml_xlsx::streaming::{StyleNumberFormat, read_streaming_styles};
        let bytes = self.read_metadata(name, u32::MAX)?;
        let cache = read_streaming_styles(&bytes).map_err(crate::js_error)?;
        let fonts = style_group(cache.fonts, crate::encode_declared_font);
        let fills = style_group(cache.fills, crate::encode_fill);
        let borders = style_group(cache.borders, crate::encode_border);
        let mut output = Vec::with_capacity(3 + cache.styles.len() * 6);
        output.extend([fonts.into(), fills.into(), borders.into()]);
        for style in cache.styles {
            let format = match style.number_format {
                StyleNumberFormat::Builtin(id) => id.into(),
                StyleNumberFormat::Custom(code) => code.into(),
            };
            let alignment = style
                .alignment
                .into_iter()
                .flat_map(|(key, value)| [key, value])
                .map(JsValue::from)
                .collect::<js_sys::Array>();
            let protection = style.protection.map_or(JsValue::UNDEFINED, |flags| {
                [JsValue::from(flags.locked), JsValue::from(flags.hidden)]
                    .into_iter()
                    .collect::<js_sys::Array>()
                    .into()
            });
            output.extend([
                format,
                style.font_id.map_or(JsValue::UNDEFINED, JsValue::from),
                style.fill_id.map_or(JsValue::UNDEFINED, JsValue::from),
                style.border_id.map_or(JsValue::UNDEFINED, JsValue::from),
                alignment.into(),
                protection,
            ]);
        }
        Ok(output)
    }

    /// Reads the hyperlink relationships for a worksheet, if the part exists.
    pub fn sheet_hyperlink_relationships(
        &mut self,
        sheet: &str,
    ) -> Result<Option<Vec<JsValue>>, JsError> {
        use ooxml_core::opc::{Relationships, TargetMode, rels_part_name};
        const HYPERLINK: &str =
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
        let part = rels_part_name(Some(sheet));
        if self.archive.index_for_name(&part).is_none() {
            return Ok(None);
        }
        let bytes = self.read_metadata(&part, u32::MAX)?;
        let relationships = Relationships::parse(Some(sheet), &bytes).map_err(zip_io_error)?;
        Ok(Some(
            relationships
                .by_type(HYPERLINK)
                .flat_map(|relationship| {
                    [
                        relationship.id.as_str().into(),
                        relationship.target.as_str().into(),
                        if relationship.target_mode == TargetMode::External {
                            "External".into()
                        } else {
                            JsValue::UNDEFINED
                        },
                    ]
                })
                .collect(),
        ))
    }

    /// Opens a stored or deflated worksheet and decodes it incrementally.
    pub fn open_rows(&mut self, name: &str) -> Result<XlsxRowStream, JsError> {
        let entry = self
            .archive
            .by_name_raw(name.as_bytes())
            .map_err(zip_io_error)?;
        if entry.encrypted() {
            return Err(JsError::new("Encrypted worksheet entries are unsupported"));
        }
        let start = entry
            .data_start()
            .ok_or_else(|| JsError::new("Missing worksheet ZIP offset"))?;
        let size = entry.compressed_size();
        if start
            .checked_add(size)
            .is_none_or(|end| end > self.source.length)
        {
            return Err(JsError::new("Worksheet ZIP data is outside the archive"));
        }
        let mut source = self.source.clone();
        source.position = start;
        let compressed = source.take(size);
        let inner: Box<dyn Read> = match entry.compression() {
            CompressionMethod::Stored => Box::new(compressed),
            CompressionMethod::Deflated => Box::new(DeflateDecoder::new(compressed)),
            _ => return Err(JsError::new("Unsupported worksheet ZIP compression")),
        };
        Ok(XlsxRowStream::from_source(CheckedEntry {
            inner,
            remaining: entry.size(),
            checksum: Hasher::new(),
            expected_checksum: entry.crc32(),
        }))
    }
}

fn style_group<T>(items: Vec<Option<T>>, encode: impl Fn(T) -> Vec<String>) -> js_sys::Array {
    items
        .into_iter()
        .map(|item| {
            item.map_or(JsValue::UNDEFINED, |item| {
                encode(item)
                    .into_iter()
                    .map(JsValue::from)
                    .collect::<js_sys::Array>()
                    .into()
            })
        })
        .collect()
}

impl XlsxZipReader {
    fn read_metadata_bytes(&mut self, name: &str, limit: u32) -> io::Result<Vec<u8>> {
        let entry = self.archive.by_name(name).map_err(io::Error::other)?;
        if entry.size() > u64::from(limit) {
            return Err(io::Error::other("ZIP metadata part exceeds its byte limit"));
        }
        let mut bytes = Vec::new();
        entry
            .take(u64::from(limit) + 1)
            .read_to_end(&mut bytes)
            .map_err(io::Error::other)?;
        if bytes.len() > limit as usize {
            return Err(io::Error::other("ZIP metadata part exceeds its byte limit"));
        }
        Ok(bytes)
    }
}
