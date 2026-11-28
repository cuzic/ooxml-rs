//! `Document`: sequential docx handle.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::{Py, PyAny};

use crate::errors::convert;
use crate::handles::{Handle, run};

/// An open document.
#[pyclass(frozen)]
pub struct Document {
    handle: Handle<ooxml_docx::Document>,
}

#[pymethods]
impl Document {
    /// Opens a document file.
    #[staticmethod]
    fn open(py: Python<'_>, path: PathBuf) -> PyResult<Self> {
        let filename = path.to_string_lossy().into_owned();
        let value = py.detach(|| ooxml_docx::Document::open(&path));
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, Some(&filename))),
        }
    }

    /// Opens a document from bytes.
    #[staticmethod]
    fn from_bytes(py: Python<'_>, bytes: Vec<u8>) -> PyResult<Self> {
        let value = py.detach(|| ooxml_docx::Document::from_bytes(bytes));
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, None)),
        }
    }

    /// Saves to a path (streaming write to a temporary file and rename).
    fn save(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let filename = path.to_string_lossy().into_owned();
        run(py, &self.handle, None, Some(&filename), |doc| doc.save(&path))
    }

    /// Serializes the document.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes: Vec<u8> = run(py, &self.handle, None, None, |doc| doc.to_bytes())?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// The main story's paragraph texts in order.
    fn paragraph_texts(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        run(py, &self.handle, None, None, |doc| {
            Ok::<_, ooxml_docx::Error>(doc.paragraphs().map(|p| p.text()).collect())
        })
    }

    /// Appends a paragraph; returns its index in the main story.
    fn append_paragraph(&self, py: Python<'_>, text: String) -> PyResult<usize> {
        run(py, &self.handle, None, None, |doc| doc.append_paragraph(&text))
    }

    /// Replaces `needle` with `replacement` (default options). With
    /// `on_refused="refuse"` one refused match refuses the whole call
    /// (nothing written); with `"skip"` the accepted matches are written and
    /// the refused ones come back as dicts (`story`, `paragraph`, `start`,
    /// `end`, `reason`).
    #[pyo3(signature = (needle, replacement, *, on_refused="refuse"))]
    fn replace(
        &self,
        py: Python<'_>,
        needle: String,
        replacement: String,
        on_refused: &str,
    ) -> PyResult<Py<PyAny>> {
        self.replace_many(py, vec![(needle, replacement)], on_refused)
    }

    /// `replace` over pairs, applied in order under one lock. Multi-pair
    /// calls serialize the document once up front (the atomicity snapshot),
    /// so they cost one serialization even on success.
    #[pyo3(signature = (pairs, *, on_refused="refuse"))]
    fn replace_many(
        &self,
        py: Python<'_>,
        pairs: Vec<(String, String)>,
        on_refused: &str,
    ) -> PyResult<Py<PyAny>> {
        use ooxml_docx::{OnRefused, ReplaceOptions};
        let mode = match on_refused {
            "refuse" => OnRefused::Refuse,
            "skip" => OnRefused::Skip,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "on_refused must be \"refuse\" or \"skip\", got {on_refused:?}"
                )));
            }
        };
        let opts = ReplaceOptions::default().on_refused(mode);
        let (replaced, skipped): (usize, Vec<Skipped>) = run(py, &self.handle, None, None, |doc| {
            // Multi-pair calls snapshot first: a later pair's refusal (or a
            // later EmptyNeedle in skip mode) must not leave earlier pairs
            // applied (Err ⇒ byte-identical). Single pairs are atomic in Rust.
            // Valid only while the binding exposes no non-byte Document
            // state (fields_opt_in, gates, high-water marks): restoring
            // from bytes resets those to reopen values. A later binding
            // PR that exposes such state must switch to a Rust-side
            // replace_many that plans before the first write.
            let snapshot = if pairs.len() > 1 { Some(doc.to_bytes()?) } else { None };
            let mut replaced = 0;
            let mut skipped = Vec::new();
            for (needle, replacement) in &pairs {
                match doc.replace(needle, replacement, &opts) {
                    Ok(report) => {
                        replaced += report.replaced;
                        skipped.extend(report.skipped.into_iter().map(Skipped::from));
                    }
                    Err(error) => {
                        if let Some(bytes) = snapshot {
                            // Round-tripping our own bytes cannot fail; a
                            // failure here is a bug and must be loud (the
                            // handle then dead-ends as HandlePoisoned).
                            *doc = ooxml_docx::Document::from_bytes(bytes)
                                .expect("snapshot reopens");
                        }
                        return Err(error);
                    }
                }
            }
            Ok::<_, ooxml_docx::Error>((replaced, skipped))
        })?;
        if mode == OnRefused::Refuse {
            Ok(replaced.into_pyobject(py)?.into_any().unbind())
        } else {
            let dicts = skipped
                .into_iter()
                .map(|s| {
                    let d = pyo3::types::PyDict::new(py);
                    d.set_item("story", s.story)?;
                    d.set_item("paragraph", s.paragraph)?;
                    d.set_item("start", s.start)?;
                    d.set_item("end", s.end)?;
                    d.set_item("reason", s.reason)?;
                    Ok::<_, PyErr>(d)
                })
                .collect::<PyResult<Vec<_>>>()?;
            Ok((replaced, dicts).into_pyobject(py)?.into_any().unbind())
        }
    }
}

/// A refused match, owned (crosses the GIL release).
struct Skipped {
    story: usize,
    paragraph: usize,
    start: usize,
    end: usize,
    reason: String,
}

impl From<ooxml_docx::Refusal> for Skipped {
    fn from(refusal: ooxml_docx::Refusal) -> Self {
        Self {
            story: refusal.para.story().get(),
            paragraph: refusal.para.index(),
            start: refusal.range.start,
            end: refusal.range.end,
            reason: refusal.reason.kind().to_owned(),
        }
    }
}
