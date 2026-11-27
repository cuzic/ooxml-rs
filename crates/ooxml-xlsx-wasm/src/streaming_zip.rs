//! Drainable ZIP output for hosts with synchronous row commits.

use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
};

use wasm_bindgen::prelude::*;
use zip::{
    CompressionMethod, DateTime, ZipWriter,
    write::{SimpleFileOptions, StreamWriter},
};

use crate::zip_io_error;

#[derive(Clone, Default)]
struct Output(Rc<RefCell<OutputBuffer>>);

#[derive(Default)]
struct OutputBuffer {
    bytes: Vec<u8>,
    passthrough: bool,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut output = self.0.borrow_mut();
        if !output.passthrough {
            output.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Streaming ZIP writer. The host drains encoded output between chunks.
#[wasm_bindgen]
pub struct XlsxZipStream {
    writer: Option<ZipWriter<StreamWriter<Output>>>,
    output: Output,
    stored: bool,
    compression_level: Option<i32>,
    timestamp: Option<DateTime>,
}

#[wasm_bindgen]
impl XlsxZipStream {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let output = Output::default();
        let writer = ZipWriter::new_stream(output.clone());
        Self {
            writer: Some(writer),
            output,
            stored: false,
            compression_level: None,
            timestamp: None,
        }
    }

    pub fn set_comment(&mut self, comment: &str) -> Result<(), JsError> {
        self.writer()?.set_comment(comment).map_err(zip_io_error)
    }

    pub fn force_zip64(&mut self) -> Result<(), JsError> {
        // An empty extensible sector makes the ZIP writer emit ZIP64 end records.
        self.writer()?.set_raw_zip64_extensible_data_sector(Box::new([]));
        Ok(())
    }

    pub fn set_timestamp(&mut self, parts: &[u16]) -> Result<(), JsError> {
        let &[year, month, day, hour, minute, second] = parts else {
            return Err(JsError::new("Expected six ZIP timestamp components"));
        };
        let timestamp = match year {
            ..1980 => Ok(DateTime::default()),
            2044.. => DateTime::from_date_and_time(2043, 12, 31, 23, 59, 58),
            _ => DateTime::from_date_and_time(year, month as u8, day as u8, hour as u8, minute as u8, second as u8),
        }.map_err(|_| JsError::new("Invalid ZIP timestamp"))?;
        self.timestamp = Some(timestamp);
        Ok(())
    }

    /// Resolves archive-wide compression and returns whether the host must
    /// pass worksheet chunks through the compressor instead of writing them.
    pub fn configure_compression(
        &mut self,
        store: bool,
        level: Option<i32>,
        zlib_level: Option<i32>,
    ) -> Result<bool, JsError> {
        if [level, zlib_level].into_iter().flatten().any(|level| !(-1..=9).contains(&level)) {
            return Err(JsError::new("Expected a ZIP level from -1 to 9"));
        }
        self.compression_level = if store {
            Some(0)
        } else {
            level.filter(|level| *level >= 0).or(zlib_level.filter(|level| *level >= 0))
        };
        Ok(self.compression_level != Some(0))
    }

    pub fn start_configured_entry(&mut self, name: &str) -> Result<(), JsError> {
        let level = self.compression_level;
        self.start_entry(name, level != Some(0), level.filter(|level| *level > 0))
    }

    pub fn start_entry(&mut self, name: &str, compressed: bool, level: Option<i32>) -> Result<(), JsError> {
        let method = if compressed {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        };
        let mut options = SimpleFileOptions::default()
            .compression_method(method)
            .compression_level(level.map(i64::from));
        if let Some(timestamp) = self.timestamp {
            options = options.last_modified_time(timestamp);
        }
        self.stored = false;
        self.writer()?
            .start_file(name, options)
            .map_err(zip_io_error)?;
        self.stored = !compressed;
        Ok(())
    }

    pub fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.writer()?.write_all(bytes).map_err(zip_io_error)
    }

    /// Accounts for bytes in a stored entry without copying them to the drain buffer.
    /// The host must emit the original bytes immediately after all drained output.
    pub fn write_stored_chunk(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        if !self.stored {
            return Err(JsError::new("Expected an active stored ZIP entry"));
        }
        if !self.output.0.borrow().bytes.is_empty() {
            return Err(JsError::new(
                "Drain ZIP output before passing through stored bytes",
            ));
        }
        let output = self.output.clone();
        let writer = self.writer()?;
        output.0.borrow_mut().passthrough = true;
        let result = writer.write_all(bytes);
        output.0.borrow_mut().passthrough = false;
        result.map_err(zip_io_error)
    }

    pub fn drain(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output.0.borrow_mut().bytes)
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, JsError> {
        self.stored = false;
        let writer = self.writer.take().ok_or_else(finalized)?;
        writer.finish().map_err(zip_io_error)?;
        Ok(self.drain())
    }
}

impl Default for XlsxZipStream {
    fn default() -> Self {
        Self::new()
    }
}

impl XlsxZipStream {
    fn writer(&mut self) -> Result<&mut ZipWriter<StreamWriter<Output>>, JsError> {
        self.writer.as_mut().ok_or_else(finalized)
    }
}

fn finalized() -> JsError {
    JsError::new("ZIP writer is already finalized")
}
