//! `Presentation`: sequential pptx handle.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::errors::convert;
use crate::handles::{Handle, run};

/// An open presentation.
#[pyclass(frozen)]
pub struct Presentation {
    handle: Handle<ooxml_pptx::Presentation>,
}

#[pymethods]
impl Presentation {
    /// Opens a presentation file.
    #[staticmethod]
    fn open(py: Python<'_>, path: PathBuf) -> PyResult<Self> {
        let filename = path.to_string_lossy().into_owned();
        let value = py.detach(|| ooxml_pptx::Presentation::open(&path));
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, Some(&filename))),
        }
    }

    /// Opens a presentation from bytes.
    #[staticmethod]
    fn from_bytes(py: Python<'_>, bytes: Vec<u8>) -> PyResult<Self> {
        let value = py.detach(|| ooxml_pptx::Presentation::from_bytes(bytes));
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, None)),
        }
    }

    /// Saves to a path (streaming write to a temporary file and rename).
    fn save(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let filename = path.to_string_lossy().into_owned();
        run(py, &self.handle, None, Some(&filename), |deck| deck.save(&path))
    }

    /// Serializes the presentation.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes: Vec<u8> = run(py, &self.handle, None, None, |deck| deck.to_bytes())?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// One text per slide: shape and table texts in document order (table
    /// cells row-major), joined by `\n`.
    fn slide_texts(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        run(py, &self.handle, None, None, |deck| {
            let count = deck.slides().count();
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let slide = deck.slide(i)?;
                let mut texts = Vec::new();
                for shape in slide.shapes() {
                    if let Some(body) = shape.text_body() {
                        texts.push(body.text());
                    }
                    if let Some(table) = shape.table() {
                        for row in table.rows() {
                            for cell in row {
                                if let Some(body) = cell.text_body() {
                                    texts.push(body.text());
                                }
                            }
                        }
                    }
                }
                out.push(texts.join("\n"));
            }
            Ok::<_, ooxml_pptx::Error>(out)
        })
    }

    /// Replaces `pattern` with `replacement` everywhere; returns how many
    /// matches were replaced. One refused match aborts the whole call
    /// (nothing written) with `("binding", "ReplaceAborted")`.
    fn replace_all(&self, py: Python<'_>, pattern: String, replacement: String) -> PyResult<usize> {
        use ooxml_pptx::{OnRefusal, ReplaceOptions};
        let opts = ReplaceOptions::default().on_refusal(OnRefusal::Abort);
        let report = run(py, &self.handle, None, None, |deck| {
            deck.replace_all(&pattern, &replacement, &opts)
        })?;
        if report.aborted {
            return Err(crate::errors::convert_binding(
                py,
                "ReplaceAborted",
                false,
                format!("replace_all aborted: {} refused match(es), nothing written", report.skipped.len()),
            ));
        }
        Ok(report.replaced.len())
    }

    /// Adds a slide from layout `layout_index` (into `layouts()` order) at
    /// `at` (appended when absent); returns the new slide's index.
    #[pyo3(signature = (layout_index, at=None))]
    fn add_slide(&self, py: Python<'_>, layout_index: usize, at: Option<usize>) -> PyResult<usize> {
        let layouts = run(py, &self.handle, None, None, |deck| deck.layouts())?;
        let layout = layouts.get(layout_index).ok_or_else(|| {
            pyo3::exceptions::PyIndexError::new_err(format!(
                "layout index {layout_index} out of range ({} layouts)", layouts.len()
            ))
        })?;
        run(py, &self.handle, None, None, |deck| {
            let before = deck.slides().count();
            deck.add_slide(layout, at)?;
            Ok::<_, ooxml_pptx::Error>(at.unwrap_or(before))
        })
    }

    /// Deletes the `index`-th slide.
    fn delete_slide(&self, py: Python<'_>, index: usize) -> PyResult<()> {
        run(py, &self.handle, None, None, |deck| deck.delete_slide(index))
    }

    /// Moves a slide.
    fn move_slide(&self, py: Python<'_>, from_index: usize, to: usize) -> PyResult<()> {
        run(py, &self.handle, None, None, |deck| deck.move_slide(from_index, to))
    }

    /// Duplicates the `index`-th slide at `at` (appended when absent);
    /// returns the new slide's index.
    #[pyo3(signature = (index, at=None))]
    fn duplicate_slide(&self, py: Python<'_>, index: usize, at: Option<usize>) -> PyResult<usize> {
        run(py, &self.handle, None, None, |deck| {
            let before = deck.slides().count();
            deck.duplicate_slide(index, at)?;
            Ok::<_, ooxml_pptx::Error>(at.unwrap_or(before))
        })
    }
}
