//! Python bindings for holos-tda.

mod atlas;
mod bipersistence;
mod circular;
mod common;
mod coverage;
mod index;
mod persistence;
mod persistent;
mod point;
mod program;
mod proof;
mod proof_checks;
mod registration;
mod synthesis;
mod topology;

use pyo3::prelude::*;

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    registration::register(module)
}
