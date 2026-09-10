//! Circular-coordinate bindings.

use pyo3::prelude::*;

use super::common::*;

pub(crate) fn circular_params(tolerance: f64, max_iterations: usize) -> CircularCoordinateParams {
    CircularCoordinateParams::default()
        .with_tolerance(tolerance)
        .with_max_iterations(max_iterations)
}

pub(crate) fn normalized_cocycle(
    graph: &SparseDistanceMatrix,
    terms: Vec<(usize, usize, i64)>,
    modulus: u32,
    scale: f64,
) -> holos_tda::Result<holos_tda::Cocycle> {
    if modulus == 0 {
        return Err(holos_tda::Error::InvalidInput(
            "circular modulus must be positive".into(),
        ));
    }
    cocycle_from_ripser_terms(
        graph,
        modulus,
        scale,
        &terms
            .into_iter()
            .map(|(u, v, coefficient)| (u, v, coefficient.rem_euclid(i64::from(modulus)) as u32))
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn circular_coordinate_record(
    coordinate: &CircularCoordinate,
) -> CircularCoordinateRecord {
    (
        coordinate.phase.clone(),
        coordinate.field_multiplier,
        coordinate.divisibility,
        coordinate.energy,
        coordinate.relative_residual,
        coordinate.iterations,
        coordinate
            .class
            .iter()
            .map(|term| (term.basis_index, term.coefficient))
            .collect(),
    )
}

pub(crate) fn circular_result(
    graph: &SparseDistanceMatrix,
    other: Option<&SparseDistanceMatrix>,
    terms: Vec<(usize, usize, i64)>,
    scale: f64,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
) -> holos_tda::Result<CircularResult> {
    let params = circular_params(tolerance, max_iterations);
    let cocycle = normalized_cocycle(graph, terms, modulus, scale)?;
    circular_result_from_cocycle(graph, other, cocycle, params)
}

pub(crate) fn circular_result_for_class(
    graph: &SparseDistanceMatrix,
    other: Option<&SparseDistanceMatrix>,
    persistent_class: PersistentClassInput,
    tolerance: f64,
    max_iterations: usize,
) -> holos_tda::Result<CircularResult> {
    let (
        birth,
        death,
        modulus,
        scale,
        terms,
        (group_id, class_id, basis_index, source_digest, class_digest),
        (provenance_birth, provenance_death, provenance_modulus, provenance_scale),
    ) = persistent_class;
    let group_id = parse_digest(&group_id, "class space identifier")?;
    let class_id = parse_digest(&class_id, "class identifier")?;
    let source_graph_digest = parse_digest(&source_digest, "graph digest")?;
    let class_digest = parse_digest(&class_digest, "class identity digest")?;
    let cocycle = normalized_cocycle(graph, terms, modulus, scale)?;
    let interval = Bar {
        dim: 1,
        birth,
        death: death.unwrap_or(f64::INFINITY),
    };
    let provenance_interval = Bar {
        dim: 1,
        birth: provenance_birth,
        death: provenance_death.unwrap_or(f64::INFINITY),
    };
    let provenance = PersistentClassProvenance::from_parts(
        source_graph_digest,
        class_digest,
        provenance_interval,
        provenance_modulus,
        provenance_scale,
    );
    let class = PersistentClass {
        id: BasisClassId::from_bytes(class_id),
        group_id: IntervalGroupId::from_bytes(group_id),
        basis_index,
        interval,
        cocycle,
        provenance: Some(provenance),
    };
    class.validate_provenance(graph)?;
    let params = circular_params(tolerance, max_iterations);
    circular_result_from_cocycle(graph, other, class.cocycle, params)
}

fn circular_result_from_cocycle(
    graph: &SparseDistanceMatrix,
    other: Option<&SparseDistanceMatrix>,
    cocycle: holos_tda::Cocycle,
    params: CircularCoordinateParams,
) -> holos_tda::Result<CircularResult> {
    let coordinate = circular_coordinate(graph, &cocycle, params)?;
    let first = circular_coordinate_record(&coordinate);
    if let Some(other) = other {
        continued_result(graph, other, &coordinate, first, params)
    } else {
        single_result(graph, &coordinate, first)
    }
}

fn parse_digest(value: &str, label: &str) -> holos_tda::Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(holos_tda::Error::InvalidInput(format!(
            "persistent class {label} must contain 64 hexadecimal characters"
        )));
    }
    let mut digest = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).map_err(|_| {
            holos_tda::Error::InvalidInput(format!("persistent class {label} is not hexadecimal"))
        })?;
        digest[index] = u8::from_str_radix(pair, 16).map_err(|_| {
            holos_tda::Error::InvalidInput(format!("persistent class {label} is not hexadecimal"))
        })?;
    }
    Ok(digest)
}

fn continued_result(
    graph: &SparseDistanceMatrix,
    other: &SparseDistanceMatrix,
    coordinate: &CircularCoordinate,
    first: CircularCoordinateRecord,
    params: CircularCoordinateParams,
) -> holos_tda::Result<CircularResult> {
    let continuation = continue_circular_coordinate(graph, coordinate, other, params)?;
    let kind = continuation_kind(continuation.topology.kind);
    let continued = continuation
        .coordinate
        .as_ref()
        .map(circular_coordinate_record);
    let artifact = CircularCoordinateArtifact::from_continuation(
        graph,
        coordinate,
        other,
        &continuation,
        params.cohomology,
    )?
    .encode()
    .map_err(|error| holos_tda::Error::InvalidInput(error.to_string()))?;
    Ok((
        artifact,
        first,
        kind.into(),
        continuation.topology.ambiguity.len(),
        continued,
    ))
}

fn continuation_kind(kind: CohomologyContinuationKind) -> &'static str {
    match kind {
        CohomologyContinuationKind::Unique => "unique",
        CohomologyContinuationKind::Ambiguous => "ambiguous",
        CohomologyContinuationKind::NoExtension => "no_extension",
        CohomologyContinuationKind::NoNonzeroContinuation => "no_nonzero_continuation",
    }
}

fn single_result(
    graph: &SparseDistanceMatrix,
    coordinate: &CircularCoordinate,
    first: CircularCoordinateRecord,
) -> holos_tda::Result<CircularResult> {
    let artifact = CircularCoordinateArtifact::from_coordinate(graph, coordinate)?
        .encode()
        .map_err(|error| holos_tda::Error::InvalidInput(error.to_string()))?;
    Ok((artifact, first, "single".into(), 0, None))
}

pub(crate) fn dense_graph(data: Vec<f64>, scale: f64) -> holos_tda::Result<SparseDistanceMatrix> {
    let lower = pdist_to_lower(data)?;
    let matrix = DistanceMatrix::from_condensed(lower)?;
    let mut triplets = Vec::new();
    for u in 0..matrix.len() {
        for v in u + 1..matrix.len() {
            let value = matrix.get(u, v);
            if value.is_finite() && value <= scale {
                triplets.push((u, v, value));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(matrix.len(), &triplets)
}

#[pyfunction]
#[pyo3(signature = (n, triplets, cocycle, scale, modulus=47, tolerance=1e-10, max_iterations=10_000, other_triplets=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn circular_sparse(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    cocycle: Vec<(usize, usize, i64)>,
    scale: f64,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    other_triplets: Option<Vec<(usize, usize, f64)>>,
) -> PyResult<CircularResult> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let other = other_triplets
            .map(|terms| SparseDistanceMatrix::from_triplets(n, &terms))
            .transpose()
            .map_err(to_err)?;
        circular_result(
            &graph,
            other.as_ref(),
            cocycle,
            scale,
            modulus,
            tolerance,
            max_iterations,
        )
        .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, persistent_class, tolerance=1e-10, max_iterations=10_000, other_triplets=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn circular_sparse_class(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    persistent_class: PersistentClassInput,
    tolerance: f64,
    max_iterations: usize,
    other_triplets: Option<Vec<(usize, usize, f64)>>,
) -> PyResult<CircularResult> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let other = other_triplets
            .map(|terms| SparseDistanceMatrix::from_triplets(n, &terms))
            .transpose()
            .map_err(to_err)?;
        circular_result_for_class(
            &graph,
            other.as_ref(),
            persistent_class,
            tolerance,
            max_iterations,
        )
        .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (data, cocycle, scale, modulus=47, tolerance=1e-10, max_iterations=10_000, other=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn circular_condensed(
    py: Python<'_>,
    data: Vec<f64>,
    cocycle: Vec<(usize, usize, i64)>,
    scale: f64,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    other: Option<Vec<f64>>,
) -> PyResult<CircularResult> {
    py.detach(|| {
        let graph = dense_graph(data, scale).map_err(to_err)?;
        let other = other
            .map(|values| dense_graph(values, scale))
            .transpose()
            .map_err(to_err)?;
        circular_result(
            &graph,
            other.as_ref(),
            cocycle,
            scale,
            modulus,
            tolerance,
            max_iterations,
        )
        .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (data, persistent_class, tolerance=1e-10, max_iterations=10_000, other=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn circular_condensed_class(
    py: Python<'_>,
    data: Vec<f64>,
    persistent_class: PersistentClassInput,
    tolerance: f64,
    max_iterations: usize,
    other: Option<Vec<f64>>,
) -> PyResult<CircularResult> {
    py.detach(|| {
        let scale = persistent_class.3;
        let graph = dense_graph(data, scale).map_err(to_err)?;
        let other = other
            .map(|values| dense_graph(values, scale))
            .transpose()
            .map_err(to_err)?;
        circular_result_for_class(
            &graph,
            other.as_ref(),
            persistent_class,
            tolerance,
            max_iterations,
        )
        .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (points, cocycle, scale, modulus=47, tolerance=1e-10, max_iterations=10_000, threads=1, other=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn circular_points(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    cocycle: Vec<(usize, usize, i64)>,
    scale: f64,
    modulus: u32,
    tolerance: f64,
    max_iterations: usize,
    threads: usize,
    other: Option<Vec<Vec<f64>>>,
) -> PyResult<CircularResult> {
    py.detach(|| {
        let graph =
            PointCloudGraph::build(&points, PointCloudParams::new(scale).with_threads(threads))
                .map_err(to_err)?;
        let other = other
            .map(|points| {
                PointCloudGraph::build(&points, PointCloudParams::new(scale).with_threads(threads))
            })
            .transpose()
            .map_err(to_err)?;
        circular_result(
            graph.matrix(),
            other.as_ref().map(PointCloudGraph::matrix),
            cocycle,
            scale,
            modulus,
            tolerance,
            max_iterations,
        )
        .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (points, persistent_class, tolerance=1e-10, max_iterations=10_000, threads=1, other=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn circular_points_class(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    persistent_class: PersistentClassInput,
    tolerance: f64,
    max_iterations: usize,
    threads: usize,
    other: Option<Vec<Vec<f64>>>,
) -> PyResult<CircularResult> {
    py.detach(|| {
        let scale = persistent_class.3;
        let graph =
            PointCloudGraph::build(&points, PointCloudParams::new(scale).with_threads(threads))
                .map_err(to_err)?;
        let other = other
            .map(|points| {
                PointCloudGraph::build(&points, PointCloudParams::new(scale).with_threads(threads))
            })
            .transpose()
            .map_err(to_err)?;
        circular_result_for_class(
            graph.matrix(),
            other.as_ref().map(PointCloudGraph::matrix),
            persistent_class,
            tolerance,
            max_iterations,
        )
        .map_err(to_err)
    })
}
