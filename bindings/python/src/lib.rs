//! Native module of the Python binding (ADR-0023 §10–§12).
//!
//! Handles (`Workbook`, `Document`, `Presentation`) are `#[pyclass(frozen)]`
//! wrappers around a `Mutex<Slot<T>>` and a `Condvar`. The mutex is only ever
//! locked with the GIL released (§11.1).

mod document;
mod errors;
mod handles;
mod presentation;
mod workbook;

use pyo3::prelude::*;

/// Crate version (matches the Python package).
#[pyfunction]
fn native_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The native module: `ooxml_rs._native`.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<workbook::Workbook>()?;
    m.add_class::<document::Document>()?;
    m.add_class::<presentation::Presentation>()?;
    m.add_function(wrap_pyfunction!(native_version, m)?)?;
    m.add_function(wrap_pyfunction!(errors::__kinds, m)?)?;
    m.add_function(wrap_pyfunction!(errors::__table_keys, m)?)?;
    Ok(())
}
