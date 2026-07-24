//! Python bindings for holos-tda. The Python-facing API lives in
//! `python/holos_tda/__init__.py`; this module stays a thin shim.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use holos_tda::{
    rips_persistence, rips_persistence_sparse, DistanceMatrix, RipsParams, SparseDistanceMatrix,
};

type Bars = Vec<(usize, f64, f64)>;

fn params(max_dim: usize, threshold: Option<f64>, modulus: u32) -> RipsParams {
    let mut p = RipsParams::new(max_dim).with_modulus(modulus);
    p.threshold = threshold;
    p
}

fn to_err(e: holos_tda::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Essential bars keep death = f64::INFINITY, which pyo3 converts to math.inf.
fn to_bars(mut diagram: holos_tda::Diagram) -> Bars {
    diagram.canonicalize();
    diagram
        .bars
        .into_iter()
        .map(|b| (b.dim, b.birth, b.death))
        .collect()
}

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=2))]
fn rips_points(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> PyResult<Bars> {
    py.detach(|| {
        let dist = DistanceMatrix::from_points(&points).map_err(to_err)?;
        rips_persistence(&dist, &params(max_dim, threshold, modulus))
            .map(to_bars)
            .map_err(to_err)
    })
}

/// SciPy's `pdist` emits the upper triangle row by row (d01, d02, ..., d12,
/// ...); the core constructor wants the lower triangle (d10, d20, d21, ...).
/// Reorder so the Python contract is the pdist one.
fn pdist_to_lower(data: Vec<f64>) -> Result<Vec<f64>, holos_tda::Error> {
    let m = data.len();
    let n = ((1.0 + 8.0 * m as f64).sqrt() as usize).div_ceil(2);
    if n * (n - 1) / 2 != m {
        return Err(holos_tda::Error::InvalidInput(format!(
            "condensed length {m} is not n(n-1)/2 for any n"
        )));
    }
    let mut lower = vec![0.0; m];
    let mut pos = 0;
    for i in 0..n {
        for j in i + 1..n {
            lower[j * (j - 1) / 2 + i] = data[pos];
            pos += 1;
        }
    }
    Ok(lower)
}

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=2))]
fn rips_condensed(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> PyResult<Bars> {
    py.detach(|| {
        let lower = pdist_to_lower(data).map_err(to_err)?;
        let dist = DistanceMatrix::from_condensed(lower).map_err(to_err)?;
        rips_persistence(&dist, &params(max_dim, threshold, modulus))
            .map(to_bars)
            .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2))]
fn rips_sparse(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> PyResult<Bars> {
    py.detach(|| {
        let dist = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        rips_persistence_sparse(&dist, &params(max_dim, threshold, modulus))
            .map(to_bars)
            .map_err(to_err)
    })
}

#[pyfunction]
fn run_cli(py: Python<'_>, argv: Vec<String>) -> i32 {
    py.detach(|| holos_tda::cli::run_cli(argv))
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(rips_points, m)?)?;
    m.add_function(wrap_pyfunction!(rips_condensed, m)?)?;
    m.add_function(wrap_pyfunction!(rips_sparse, m)?)?;
    m.add_function(wrap_pyfunction!(run_cli, m)?)?;
    m.add("__version__", holos_tda::VERSION)?;
    m.add("GIT_HASH", holos_tda::GIT_HASH)?;
    Ok(())
}
