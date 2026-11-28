//! Handles: `Mutex<Slot<T>>` + `Condvar`, GIL released while blocked (§11.1).
//!
//! Every method converts arguments with the GIL, releases it (`detach`),
//! locks, runs the Rust call, unlocks, and converts the owned result back
//! with the GIL. Never holding the GIL while blocked rules out the
//! GIL/mutex deadlock. A poisoned mutex (a panic crossed it) dead-ends the
//! handle with `("binding", "HandlePoisoned")` (§11.2).

use std::sync::{Condvar, Mutex};

use pyo3::prelude::*;

use crate::errors::{CrateError, convert, convert_binding};

/// The locked state of a handle. (U8 adds the transaction owner/seq here.)
pub struct Slot<T> {
    /// The Rust value.
    pub value: T,
}

/// A `#[pyclass(frozen)]` handle body.
pub struct Handle<T> {
    /// The value. Only ever locked with the GIL released.
    pub mutex: Mutex<Slot<T>>,
    /// U8: transactions wait here. Present now so the shape is stable.
    #[allow(dead_code)]
    pub condvar: Condvar,
}

impl<T> Handle<T> {
    /// A handle around an owned value.
    pub fn new(value: T) -> Self {
        Self { mutex: Mutex::new(Slot { value }), condvar: Condvar::new() }
    }
}

/// Runs `f` on the locked value with the GIL released, then converts.
/// `sheet`/`filename` annotate the error (§11.3).
pub fn run<T, R, E, F>(
    py: Python<'_>,
    handle: &Handle<T>,
    sheet: Option<&str>,
    filename: Option<&str>,
    f: F,
) -> PyResult<R>
where
    T: Send,
    F: FnOnce(&mut T) -> Result<R, E> + Send,
    R: Send,
    E: CrateError + Send,
{
    enum Outcome<R, E> {
        Ok(R),
        Err(E),
        Poisoned,
    }
    // `Send`: only `&Handle` (Sync) and owned values cross into `detach`.
    let outcome = py.detach(|| match handle.mutex.lock() {
        Ok(mut guard) => match f(&mut guard.value) {
            Ok(value) => Outcome::Ok(value),
            Err(error) => Outcome::Err(error),
        },
        Err(_) => Outcome::Poisoned,
    });
    match outcome {
        Outcome::Ok(value) => Ok(value),
        Outcome::Err(error) => Err(convert(py, error, sheet, filename)),
        Outcome::Poisoned => {
            Err(convert_binding(py, "HandlePoisoned", true, "the handle is poisoned by a panic; reopen it".into()))
        }
    }
}
