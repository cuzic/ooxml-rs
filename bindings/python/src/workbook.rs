//! `Workbook`: sequential xlsx handle (U8 moves it to journal sessions).

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::errors::convert;
use crate::handles::{Handle, run};

/// An open workbook. Sequential in U7 (journal sessions land in U8).
#[pyclass(frozen)]
pub struct Workbook {
    handle: Handle<ooxml_xlsx::Workbook>,
}

#[pymethods]
impl Workbook {
    /// Opens a workbook file.
    #[staticmethod]
    fn open(py: Python<'_>, path: PathBuf) -> PyResult<Self> {
        let filename = path.to_string_lossy().into_owned();
        let value = py.detach(|| ooxml_xlsx::Workbook::open(&path));
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, Some(&filename))),
        }
    }

    /// Opens a workbook from bytes.
    #[staticmethod]
    fn from_bytes(py: Python<'_>, bytes: Vec<u8>) -> PyResult<Self> {
        let value = py.detach(|| ooxml_xlsx::Workbook::from_bytes(bytes));
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, None)),
        }
    }

    /// A new empty workbook.
    #[staticmethod]
    fn new(py: Python<'_>) -> PyResult<Self> {
        let value = py.detach(ooxml_xlsx::Workbook::new);
        match value {
            Ok(value) => Ok(Self { handle: Handle::new(value) }),
            Err(error) => Err(convert(py, error, None, None)),
        }
    }

    /// Saves to a path (streaming write to a temporary file and rename).
    fn save(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let filename = path.to_string_lossy().into_owned();
        run(py, &self.handle, None, Some(&filename), |wb| wb.save(&path))
    }

    /// Serializes the workbook.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes: Vec<u8> = run(py, &self.handle, None, None, |wb| wb.to_bytes())?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Sheet names in workbook order (every kind of sheet).
    fn sheet_names(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        run(py, &self.handle, None, None, |wb| {
            Ok::<_, ooxml_xlsx::Error>(wb.sheet_names().map(str::to_owned).collect())
        })
    }

    /// Arms a fault before an apply step (fault-injection test builds only).
    /// Returns whether a fault was already armed. Reads never trip it.
    #[cfg(feature = "fault-injection")]
    #[doc(hidden)]
    fn __inject_fault(&self, py: Python<'_>, step: &str, panic: bool) -> PyResult<bool> {
        use ooxml_xlsx::ApplyStep;
        let step = match step {
            "RemoveCalcChain" => ApplyStep::RemoveCalcChain,
            "CalcPr" => ApplyStep::CalcPr,
            "Strip" => ApplyStep::Strip,
            "HandOff" => ApplyStep::HandOff,
            "Registries" => ApplyStep::Registries,
            "Target" => ApplyStep::Target,
            "LaterTarget" => ApplyStep::LaterTarget,
            "Registry" => ApplyStep::Registry,
            "StaleRegistryPlan" => ApplyStep::StaleRegistryPlan,
            "Index" => ApplyStep::Index,
            "StructuralApply" => ApplyStep::StructuralApply,
            "StructuralRebuild" => ApplyStep::StructuralRebuild,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "unknown apply step {step:?}"
                )));
            }
        };
        run(py, &self.handle, None, None, |wb| {
            Ok::<_, ooxml_xlsx::Error>(wb.__inject_fault(step, panic))
        })
    }
}
