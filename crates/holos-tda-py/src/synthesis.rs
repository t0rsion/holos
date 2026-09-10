//! Cohomology synthesis bindings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use super::common::*;

#[pyfunction]
#[pyo3(signature = (n, states, candidates, dimension, scale, max_rank, max_edits, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_fixed_cohomology(
    py: Python<'_>,
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    candidates: Vec<(usize, usize, u64)>,
    dimension: usize,
    scale: f64,
    max_rank: usize,
    max_edits: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<SynthesisRecord> {
    py.detach(|| {
        let mut obligations = Vec::new();
        for (step, triplets) in states.into_iter().enumerate() {
            let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let space = cohomology_space(
                &graph,
                dimension,
                scale,
                modulus,
                CohomologyLimits::default(),
            )
            .map_err(to_err)?;
            if space.rank() <= max_rank {
                continue;
            }
            let target = space.full_subspace();
            obligations.push(
                SynthesisState::from_subspace(
                    0,
                    step as u64,
                    &graph,
                    scale,
                    &space,
                    &target,
                    max_rank,
                )
                .map_err(to_err)?,
            );
        }
        let specification =
            TopologicalSpecification::new(n, dimension, scale, modulus, obligations);
        let actions = synthesis_actions(candidates, &specification)?;
        build_synthesis_record(specification, actions, max_edits, oracle_limit, node_limit)
    })
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, candidates, dimension, scale, max_rank, max_edits, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_affine_cohomology(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    candidates: Vec<(usize, usize, u64)>,
    dimension: usize,
    scale: f64,
    max_rank: usize,
    max_edits: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<SynthesisRecord> {
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
        let specification = TopologicalSpecification::from_kinetic_rank_ceiling(
            &trajectory,
            0,
            dimension,
            scale,
            modulus,
            max_rank,
            CohomologyLimits::default(),
        )
        .map_err(to_err)?;
        let actions = synthesis_actions(candidates, &specification)?;
        build_synthesis_record(specification, actions, max_edits, oracle_limit, node_limit)
    })
}

pub(crate) fn synthesis_actions(
    candidates: Vec<(usize, usize, u64)>,
    specification: &TopologicalSpecification,
) -> PyResult<Vec<SynthesisAction>> {
    if specification.states().is_empty() {
        return Ok(Vec::new());
    }
    let mut actions = candidates
        .into_iter()
        .map(|(u, v, cost)| SynthesisAction::throughout(u, v, cost, specification))
        .collect::<Vec<_>>();
    actions.sort();
    let before = actions.len();
    actions.dedup_by_key(|action| action.edge);
    if actions.len() != before {
        return Err(PyValueError::new_err("candidates repeat an edge"));
    }
    Ok(actions)
}

pub(crate) fn build_synthesis_record(
    specification: TopologicalSpecification,
    actions: Vec<SynthesisAction>,
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<SynthesisRecord> {
    let limits = SynthesisLimits::default()
        .with_max_oracle_calls(oracle_limit)
        .with_max_search_nodes(node_limit);
    let artifact =
        SynthesisArtifact::build(specification, actions, max_edits, limits).map_err(to_err)?;
    let bytes = artifact.encode(limits).map_err(to_err)?;
    Ok((
        bytes,
        artifact.status().to_string(),
        artifact
            .selected()
            .iter()
            .map(|position| {
                let action = &artifact.actions()[*position];
                (action.edge.u, action.edge.v, action.cost)
            })
            .collect(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.producer_search_nodes(),
        artifact.proof_nodes(),
        artifact.proof_topology_checks(),
        artifact.before_ranks().to_vec(),
        artifact.after_ranks().to_vec(),
    ))
}
