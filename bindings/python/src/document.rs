//! `Document`: sequential docx handle.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;

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
}
