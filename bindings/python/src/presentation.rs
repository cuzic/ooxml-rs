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
}
