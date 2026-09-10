//! Coverage evaluation and synthesis bindings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use super::common::*;

#[pyfunction]
#[pyo3(signature = (n, triplets, active, fence, broadcast_radius, sensing_radius, modulus=2))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_relative_coverage(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    active: Vec<usize>,
    fence: Vec<usize>,
    broadcast_radius: f64,
    sensing_radius: f64,
    modulus: u32,
) -> PyResult<RelativeCoverageRecord> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let fence = CoverageFence::new(fence).map_err(to_err)?;
        let model = PlanarCoverageModel::new(broadcast_radius, sensing_radius).map_err(to_err)?;
        let evaluation = evaluate_planar_coverage(
            &graph,
            &active,
            &fence,
            modulus,
            model,
            CoverageLimits::default(),
        )
        .map_err(to_err)?;
        Ok((
            evaluation.criterion_holds,
            evaluation
                .witness
                .into_iter()
                .map(|term| (term.a, term.b, term.c, term.coefficient))
                .collect(),
            evaluation.active_edges,
            evaluation.active_triangles,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, states, fence, base, failable, candidates, broadcast_radius, sensing_radius, failure_budget, max_activations, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_finite_coverage(
    py: Python<'_>,
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    candidates: Vec<CoverageCandidateInput>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    max_activations: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    py.detach(|| {
        let specification = finite_coverage_specification(
            n,
            states,
            fence,
            base,
            failable,
            broadcast_radius,
            sensing_radius,
            failure_budget,
            modulus,
        )?;
        let actions = coverage_actions(candidates, &specification)?;
        build_coverage_record(
            specification,
            actions,
            max_activations,
            oracle_limit,
            node_limit,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (n, states, coordinates, fence, base, failable, candidates, broadcast_radius, sensing_radius, failure_budget, max_activations, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_geometric_coverage(
    py: Python<'_>,
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    coordinates: Vec<Vec<(f64, f64)>>,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    candidates: Vec<CoverageCandidateInput>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    max_activations: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    py.detach(|| {
        let specification = finite_coverage_specification(
            n,
            states,
            fence,
            base,
            failable,
            broadcast_radius,
            sensing_radius,
            failure_budget,
            modulus,
        )?;
        let actions = coverage_actions(candidates, &specification)?;
        let geometry = CoverageGeometry::new(
            &specification,
            coordinates
                .into_iter()
                .map(|state| {
                    state
                        .into_iter()
                        .map(|(x, y)| PlanarPoint::new(x, y))
                        .collect::<holos_tda::Result<Vec<_>>>()
                })
                .collect::<holos_tda::Result<Vec<_>>>()
                .map_err(to_err)?,
            CoverageGeometryLimits::default(),
        )
        .map_err(to_err)?;
        build_geometric_coverage_record(
            specification,
            actions,
            geometry,
            max_activations,
            oracle_limit,
            node_limit,
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finite_coverage_specification(
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    modulus: u32,
) -> PyResult<CoverageSpecification> {
    let model = PlanarCoverageModel::new(broadcast_radius, sensing_radius).map_err(to_err)?;
    let fence = CoverageFence::new(fence).map_err(to_err)?;
    let base = coverage_base(fence.vertices(), &base);
    let states = states
        .into_iter()
        .enumerate()
        .map(|(step, triplets)| {
            let graph = SparseDistanceMatrix::from_triplets(n, &triplets)?;
            CoverageState::new(0, step as u64, &graph, base.clone(), broadcast_radius)
        })
        .collect::<holos_tda::Result<Vec<_>>>()
        .map_err(to_err)?;
    CoverageSpecification::new(
        n,
        model,
        modulus,
        fence,
        failable,
        failure_budget,
        states,
        CoverageLimits::default(),
    )
    .map_err(to_err)
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, fence, base, failable, candidates, broadcast_radius, sensing_radius, failure_budget, max_activations, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_affine_coverage(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    candidates: Vec<CoverageCandidateInput>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    max_activations: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    py.detach(|| {
        let trajectory = KineticFiltration::new(
            n,
            edges
                .into_iter()
                .map(|(u, v, intercept, velocity)| KineticEdge {
                    u,
                    v,
                    intercept,
                    velocity,
                })
                .collect(),
            start,
            end,
            KineticLimits::default(),
        )
        .map_err(to_err)?;
        let model = PlanarCoverageModel::new(broadcast_radius, sensing_radius).map_err(to_err)?;
        let fence = CoverageFence::new(fence).map_err(to_err)?;
        let base = coverage_base(fence.vertices(), &base);
        let specification = CoverageSpecification::from_kinetic(
            &trajectory,
            0,
            model,
            modulus,
            fence,
            failable,
            failure_budget,
            base,
            CoverageLimits::default(),
        )
        .map_err(to_err)?;
        let actions = coverage_actions(candidates, &specification)?;
        build_coverage_record(
            specification,
            actions,
            max_activations,
            oracle_limit,
            node_limit,
        )
    })
}

pub(crate) fn coverage_base(fence: &[usize], additional: &[usize]) -> Vec<usize> {
    let mut base = fence.iter().chain(additional).copied().collect::<Vec<_>>();
    base.sort_unstable();
    base.dedup();
    base
}

pub(crate) fn coverage_actions(
    candidates: Vec<CoverageCandidateInput>,
    specification: &CoverageSpecification,
) -> PyResult<Vec<CoverageAction>> {
    let mut actions = candidates
        .into_iter()
        .map(|(vertex, cost, states)| {
            CoverageAction::new(
                vertex,
                cost,
                states.unwrap_or_else(|| (0..specification.states().len()).collect()),
            )
        })
        .collect::<Vec<_>>();
    actions.sort_by_key(|action| action.vertex);
    if actions
        .windows(2)
        .any(|pair| pair[0].vertex == pair[1].vertex)
    {
        return Err(PyValueError::new_err(
            "coverage candidates repeat a sensor vertex",
        ));
    }
    Ok(actions)
}

pub(crate) fn build_coverage_record(
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    let limits = CoverageSynthesisLimits::default()
        .with_max_oracle_calls(oracle_limit)
        .with_max_search_nodes(node_limit);
    let artifact =
        CoverageSynthesisArtifact::build(specification, actions, max_activations, limits)
            .map_err(to_err)?;
    let bytes = artifact.encode(limits).map_err(to_err)?;
    Ok(coverage_record(&artifact, bytes))
}

pub(crate) fn build_geometric_coverage_record(
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    geometry: CoverageGeometry,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    let limits = CoverageSynthesisLimits::default()
        .with_max_oracle_calls(oracle_limit)
        .with_max_search_nodes(node_limit);
    let artifact =
        CoverageSynthesisArtifact::build(specification, actions, max_activations, limits)
            .map_err(to_err)?;
    let mut record = coverage_record(&artifact, Vec::new());
    let geometry_limits = CoverageGeometryLimits::default();
    record.0 = GeometryBoundCoverageArtifact::build(artifact, geometry, limits, geometry_limits)
        .map_err(to_err)?
        .encode(
            limits,
            geometry_limits,
            GeometryBoundCoverageDecodeLimits::default(),
        )
        .map_err(to_err)?;
    Ok(record)
}

pub(crate) fn coverage_record(
    artifact: &CoverageSynthesisArtifact,
    bytes: Vec<u8>,
) -> CoverageRecord {
    (
        bytes,
        artifact.status().to_string(),
        artifact
            .selected()
            .iter()
            .map(|index| {
                let action = &artifact.actions()[*index];
                (action.vertex, action.cost, action.states().to_vec())
            })
            .collect(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.producer_search_nodes(),
        artifact.proof_nodes(),
        artifact.proof_topology_checks(),
        artifact.selected_failure_checks(),
        artifact.minimum_witness_triangles(),
        artifact.specification().states().len(),
    )
}
