//! Python bindings for source-bound persistent classes and coordinates.

use pyo3::prelude::*;

use holos_tda::persistent_class_artifact::{
    PersistenceCycleTerm, PersistenceTriangleTerm, PersistentClassArtifact,
};
use holos_tda::persistent_coordinate_artifact::PersistentCoordinateArtifact;
use holos_tda::{
    CriticalPair, DistanceMatrix, IntegralCocycleTerm, RipsParams, SparseDistanceMatrix,
};

use super::circular::circular_params;
use super::common::*;

type CycleRecord = Vec<(usize, usize, u32)>;
type ChainRecord = Vec<(Vec<usize>, u32)>;
type PersistentClassResult = (
    Vec<u8>,
    ClassRecord,
    CriticalRecord,
    CycleRecord,
    ChainRecord,
);
type SelectedCoordinateRecord = (
    Vec<f64>,
    u32,
    u64,
    f64,
    f64,
    f64,
    usize,
    Vec<(usize, usize, i64)>,
    Vec<f64>,
    f64,
);
type PersistentCoordinateResult = (
    Vec<u8>,
    ClassRecord,
    CriticalRecord,
    CycleRecord,
    ChainRecord,
    SelectedCoordinateRecord,
);

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_class_artifacts(module)?;
    register_coordinate_artifacts(module)
}

fn register_class_artifacts(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(persistent_class_sparse, module)?)?;
    module.add_function(wrap_pyfunction!(persistent_class_condensed, module)?)?;
    module.add_function(wrap_pyfunction!(persistent_class_points, module)?)?;
    module.add_function(wrap_pyfunction!(persistent_class_square, module)?)?;
    Ok(())
}

fn register_coordinate_artifacts(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(persistent_circular_sparse, module)?)?;
    module.add_function(wrap_pyfunction!(persistent_circular_condensed, module)?)?;
    module.add_function(wrap_pyfunction!(persistent_circular_points, module)?)?;
    module.add_function(wrap_pyfunction!(persistent_circular_square, module)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, space_index=0, basis_index=0))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_class_sparse(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    space_index: usize,
    basis_index: usize,
) -> PyResult<PersistentClassResult> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        build_class(
            &graph,
            max_dim,
            threshold,
            modulus,
            threads,
            space_index,
            basis_index,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=2, space_index=0, basis_index=0))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_class_condensed(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    space_index: usize,
    basis_index: usize,
) -> PyResult<PersistentClassResult> {
    py.detach(|| {
        let graph = condensed_graph(data).map_err(to_err)?;
        build_class(
            &graph,
            max_dim,
            threshold,
            modulus,
            1,
            space_index,
            basis_index,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=2, threads=1, space_index=0, basis_index=0))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_class_points(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    space_index: usize,
    basis_index: usize,
) -> PyResult<PersistentClassResult> {
    py.detach(|| {
        let graph = points_graph(&points).map_err(to_err)?;
        build_class(
            &graph,
            max_dim,
            threshold,
            modulus,
            threads,
            space_index,
            basis_index,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (distances, max_dim=1, threshold=None, modulus=2, space_index=0, basis_index=0))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_class_square(
    py: Python<'_>,
    distances: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    space_index: usize,
    basis_index: usize,
) -> PyResult<PersistentClassResult> {
    py.detach(|| {
        let graph = square_graph(distances).map_err(to_err)?;
        build_class(
            &graph,
            max_dim,
            threshold,
            modulus,
            1,
            space_index,
            basis_index,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=47, tolerance=1e-10, max_iterations=10_000, space_index=0, basis_index=0, integral_lift=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_circular_sparse(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    space_index: usize,
    basis_index: usize,
    integral_lift: Option<Vec<(usize, usize, i64)>>,
) -> PyResult<PersistentCoordinateResult> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        build_coordinate(
            &graph,
            max_dim,
            threshold,
            modulus,
            1,
            tolerance,
            max_iterations,
            space_index,
            basis_index,
            integral_lift,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=47, tolerance=1e-10, max_iterations=10_000, space_index=0, basis_index=0, integral_lift=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_circular_condensed(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    space_index: usize,
    basis_index: usize,
    integral_lift: Option<Vec<(usize, usize, i64)>>,
) -> PyResult<PersistentCoordinateResult> {
    py.detach(|| {
        let graph = condensed_graph(data).map_err(to_err)?;
        build_coordinate(
            &graph,
            max_dim,
            threshold,
            modulus,
            1,
            tolerance,
            max_iterations,
            space_index,
            basis_index,
            integral_lift,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=47, tolerance=1e-10, max_iterations=10_000, threads=1, space_index=0, basis_index=0, integral_lift=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_circular_points(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    threads: usize,
    space_index: usize,
    basis_index: usize,
    integral_lift: Option<Vec<(usize, usize, i64)>>,
) -> PyResult<PersistentCoordinateResult> {
    py.detach(|| {
        let graph = points_graph(&points).map_err(to_err)?;
        build_coordinate(
            &graph,
            max_dim,
            threshold,
            modulus,
            threads,
            tolerance,
            max_iterations,
            space_index,
            basis_index,
            integral_lift,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (distances, max_dim=1, threshold=None, modulus=47, tolerance=1e-10, max_iterations=10_000, space_index=0, basis_index=0, integral_lift=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn persistent_circular_square(
    py: Python<'_>,
    distances: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    space_index: usize,
    basis_index: usize,
    integral_lift: Option<Vec<(usize, usize, i64)>>,
) -> PyResult<PersistentCoordinateResult> {
    py.detach(|| {
        let graph = square_graph(distances).map_err(to_err)?;
        build_coordinate(
            &graph,
            max_dim,
            threshold,
            modulus,
            1,
            tolerance,
            max_iterations,
            space_index,
            basis_index,
            integral_lift,
        )
    })
}

fn build_class(
    graph: &SparseDistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    space_index: usize,
    basis_index: usize,
) -> PyResult<PersistentClassResult> {
    let params = rips_params(max_dim, threshold, modulus, threads);
    let artifact = PersistentClassArtifact::build(
        graph,
        &params,
        space_index,
        basis_index,
        CertificateLimits::default(),
    )
    .map_err(display_err)?;
    persistent_class_result(&artifact)
}

#[allow(clippy::too_many_arguments)]
fn build_coordinate(
    graph: &SparseDistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    tolerance: f64,
    max_iterations: usize,
    space_index: usize,
    basis_index: usize,
    integral_lift: Option<Vec<(usize, usize, i64)>>,
) -> PyResult<PersistentCoordinateResult> {
    let params = rips_params(max_dim, threshold, modulus, threads);
    let class = PersistentClassArtifact::build(
        graph,
        &params,
        space_index,
        basis_index,
        CertificateLimits::default(),
    )
    .map_err(display_err)?;
    let integral = integral_lift.map(|terms| {
        terms
            .into_iter()
            .map(|(u, v, coefficient)| IntegralCocycleTerm { u, v, coefficient })
            .collect::<Vec<_>>()
    });
    let coordinate = PersistentCoordinateArtifact::build(
        &class,
        circular_params(tolerance, max_iterations),
        integral.as_deref(),
    )
    .map_err(to_err)?;
    persistent_coordinate_result(&coordinate)
}

fn rips_params(max_dim: usize, threshold: Option<f64>, modulus: u32, threads: usize) -> RipsParams {
    let mut params = RipsParams::new(max_dim)
        .with_modulus(modulus)
        .with_threads(threads);
    params.threshold = threshold;
    params
}

fn persistent_class_result(artifact: &PersistentClassArtifact) -> PyResult<PersistentClassResult> {
    let bytes = artifact
        .encode(CertificateLimits::default())
        .map_err(display_err)?;
    let (class, critical_pair, cycle, bounding_chain) = class_records(
        artifact.class(),
        artifact.critical_pair(),
        artifact.cycle(),
        artifact.bounding_chain(),
    );
    Ok((bytes, class, critical_pair, cycle, bounding_chain))
}

fn persistent_coordinate_result(
    artifact: &PersistentCoordinateArtifact,
) -> PyResult<PersistentCoordinateResult> {
    let bytes = artifact
        .encode(CertificateLimits::default())
        .map_err(display_err)?;
    let (class, critical_pair, cycle, bounding_chain) = class_records(
        artifact.class(),
        artifact.critical_pair(),
        artifact.cycle(),
        artifact.bounding_chain(),
    );
    Ok((
        bytes,
        class,
        critical_pair,
        cycle,
        bounding_chain,
        selected_coordinate_record(artifact),
    ))
}

fn class_records(
    class: &holos_tda::PersistentClass,
    critical_pair: &CriticalPair,
    cycle: &[PersistenceCycleTerm],
    bounding_chain: &[PersistenceTriangleTerm],
) -> (ClassRecord, CriticalRecord, CycleRecord, ChainRecord) {
    (
        class_record(class.clone()),
        critical_pair_record(critical_pair),
        cycle_record(cycle),
        chain_record(bounding_chain),
    )
}

fn critical_pair_record(pair: &CriticalPair) -> CriticalRecord {
    (
        pair.birth.vertices.clone(),
        pair.birth.value,
        pair.death
            .as_ref()
            .map(|simplex| (simplex.vertices.clone(), simplex.value)),
    )
}

fn cycle_record(cycle: &[PersistenceCycleTerm]) -> CycleRecord {
    cycle
        .iter()
        .map(|term| (term.u, term.v, term.coefficient))
        .collect()
}

fn chain_record(chain: &[PersistenceTriangleTerm]) -> ChainRecord {
    chain
        .iter()
        .map(|term| (term.vertices.to_vec(), term.coefficient))
        .collect()
}

fn selected_coordinate_record(artifact: &PersistentCoordinateArtifact) -> SelectedCoordinateRecord {
    (
        artifact.phase().to_vec(),
        artifact.field_multiplier(),
        artifact.divisibility(),
        artifact.energy(),
        artifact.max_residual(),
        artifact.relative_residual(),
        artifact.iterations(),
        artifact
            .integral()
            .iter()
            .map(|term| (term.u, term.v, term.coefficient))
            .collect(),
        artifact.potential().to_vec(),
        artifact.tolerance(),
    )
}

fn points_graph(points: &[Vec<f64>]) -> holos_tda::Result<SparseDistanceMatrix> {
    // Point inputs use the core Euclidean metric. Keep every finite computed
    // pair because the artifact binds source data, not a geometry claim.
    dense_graph(DistanceMatrix::from_points(points)?)
}

fn condensed_graph(data: Vec<f64>) -> holos_tda::Result<SparseDistanceMatrix> {
    let lower = pdist_to_lower(data)?;
    let matrix = DistanceMatrix::from_condensed(lower)?;
    dense_graph(matrix)
}

fn square_graph(distances: Vec<Vec<f64>>) -> holos_tda::Result<SparseDistanceMatrix> {
    let size = distances.len();
    validate_square_distances(&distances, size)?;
    let mut triplets = Vec::new();
    for (u, row) in distances.iter().enumerate() {
        for (v, &distance) in row.iter().enumerate().skip(u + 1) {
            if distance.is_finite() {
                triplets.push((u, v, distance));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(size, &triplets)
}

fn validate_square_distances(distances: &[Vec<f64>], size: usize) -> holos_tda::Result<()> {
    for (row_index, row) in distances.iter().enumerate() {
        if row.len() != size {
            return Err(holos_tda::Error::InvalidInput(
                "distances must be a square matrix".into(),
            ));
        }
        for (column_index, &distance) in row.iter().enumerate() {
            if distance.is_nan() || distance < 0.0 {
                return Err(holos_tda::Error::InvalidDistance(format!(
                    "distance at ({row_index}, {column_index}) must be non-negative and not NaN, got {distance}"
                )));
            }
        }
        if row[row_index] != 0.0 {
            return Err(holos_tda::Error::InvalidDistance(format!(
                "distance matrix diagonal at ({row_index}, {row_index}) must be zero"
            )));
        }
    }
    for (row_index, row) in distances.iter().enumerate() {
        for (column, &upper) in row.iter().enumerate().skip(row_index + 1) {
            if upper != distances[column][row_index] {
                return Err(holos_tda::Error::InvalidInput(format!(
                    "distance matrix is not symmetric at ({row_index}, {column})"
                )));
            }
        }
    }
    Ok(())
}

fn dense_graph(matrix: DistanceMatrix) -> holos_tda::Result<SparseDistanceMatrix> {
    let size = matrix.len();
    let mut triplets = Vec::new();
    for u in 0..size {
        for v in u + 1..size {
            let distance = matrix.get(u, v);
            if distance.is_finite() {
                triplets.push((u, v, distance));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(matrix.len(), &triplets)
}
