//! Native module of the Python binding (ADR-0023 §10–§12).
//!
//! Handles (`Workbook`, `Document`, `Presentation`) are `#[pyclass(frozen)]`
//! wrappers around a `Mutex<Slot<T>>` and a `Condvar`. The mutex is only ever
//! locked with the GIL released (§11.1).

use pyo3::prelude::*;

/// The native module: `ooxml_rs._native`.
#[pymodule]
mod _native {
    use super::*;

    /// Crate version (matches the Python package).
    #[pyfunction]
    fn native_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}
