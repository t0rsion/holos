//! Single-parameter persistence bindings.

use pyo3::prelude::*;

use super::common::*;

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn rips_points(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Bars> {
    py.detach(|| {
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        match threshold {
            Some(threshold) => {
                let graph = PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_threads(threads),
                )
                .map_err(to_err)?;
                rips_persistence_sparse(graph.matrix(), &params)
                    .map(to_bars)
                    .map_err(to_err)
            }
            None => {
                let dist = DistanceMatrix::from_points(&points).map_err(to_err)?;
                rips_persistence(&dist, &params)
                    .map(to_bars)
                    .map_err(to_err)
            }
        }
    })
}

/// Reorder SciPy `pdist` data into the core lower-triangle layout.
///
/// `pdist` emits the upper triangle row by row (d01, d02, ..., d12, ...).
/// The constructor stores the lower triangle (d10, d20, d21, ...).
/// The Python contract is the pdist layout.

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn rips_condensed(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Bars> {
    py.detach(|| {
        let lower = pdist_to_lower(data).map_err(to_err)?;
        let dist = DistanceMatrix::from_condensed(lower).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence(&dist, &params)
            .map(to_bars)
            .map_err(to_err)
    })
}

// Keyword arguments match the Python signature.
#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
pub(crate) fn rips_sparse(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Bars> {
    py.detach(|| {
        let dist = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence_sparse(&dist, &params)
            .map(to_bars)
            .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn rips_points_classes(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Explained> {
    py.detach(|| {
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        match threshold {
            Some(threshold) => {
                let graph = PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_threads(threads),
                )
                .map_err(to_err)?;
                rips_persistence_with_classes_sparse(graph.matrix(), &params)
                    .and_then(to_explained)
                    .map_err(to_err)
            }
            None => {
                let dist = DistanceMatrix::from_points(&points).map_err(to_err)?;
                rips_persistence_with_classes(&dist, &params)
                    .and_then(to_explained)
                    .map_err(to_err)
            }
        }
    })
}

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn rips_condensed_classes(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Explained> {
    py.detach(|| {
        let lower = pdist_to_lower(data).map_err(to_err)?;
        let dist = DistanceMatrix::from_condensed(lower).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence_with_classes(&dist, &params)
            .and_then(to_explained)
            .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn rips_sparse_classes(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Explained> {
    py.detach(|| {
        let dist = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence_with_classes_sparse(&dist, &params)
            .and_then(to_explained)
            .map_err(to_err)
    })
}
