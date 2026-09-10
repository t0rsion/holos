//! Fixed and kinetic cohomology bindings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use super::common::*;

#[pyfunction]
#[pyo3(signature = (n, triplets, dimension, scale, modulus=2))]
pub(crate) fn fixed_cohomology(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    dimension: usize,
    scale: f64,
    modulus: u32,
) -> PyResult<FixedCohomologyRecord> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let space = cohomology_space(
            &graph,
            dimension,
            scale,
            modulus,
            CohomologyLimits::default(),
        )
        .map_err(to_err)?;
        Ok((
            space.id().to_string(),
            space.simplex_counts().to_vec(),
            space
                .basis()
                .iter()
                .map(|class| {
                    (
                        class.id.to_string(),
                        class
                            .terms
                            .iter()
                            .map(|term| (term.simplex.clone(), term.coefficient))
                            .collect(),
                    )
                })
                .collect(),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, old_triplets, new_triplets, dimension, scale, modulus=2))]
pub(crate) fn relate_fixed_cohomology(
    py: Python<'_>,
    n: usize,
    old_triplets: Vec<(usize, usize, f64)>,
    new_triplets: Vec<(usize, usize, f64)>,
    dimension: usize,
    scale: f64,
    modulus: u32,
) -> PyResult<CohomologyRelationRecord> {
    py.detach(|| {
        let old_graph = SparseDistanceMatrix::from_triplets(n, &old_triplets).map_err(to_err)?;
        let new_graph = SparseDistanceMatrix::from_triplets(n, &new_triplets).map_err(to_err)?;
        let limits = CohomologyLimits::default();
        let old =
            cohomology_space(&old_graph, dimension, scale, modulus, limits).map_err(to_err)?;
        let new =
            cohomology_space(&new_graph, dimension, scale, modulus, limits).map_err(to_err)?;
        let relation =
            cohomology_relation(&old_graph, &old, &new_graph, &new, limits).map_err(to_err)?;
        let basis = relation
            .basis
            .iter()
            .map(|vector| {
                (
                    vector
                        .old
                        .iter()
                        .map(|term| (term.class.to_string(), term.coefficient))
                        .collect(),
                    vector
                        .new
                        .iter()
                        .map(|term| (term.class.to_string(), term.coefficient))
                        .collect(),
                )
            })
            .collect();
        Ok((
            relation.old_rank,
            relation.new_rank,
            relation.old_image_rank,
            relation.new_image_rank,
            relation.old_kernel_rank,
            relation.new_kernel_rank,
            relation.relation_rank,
            relation.is_isomorphism(),
            basis,
        ))
    })
}

pub(crate) fn affine_kind(kind: &KineticEventKind) -> String {
    match kind {
        KineticEventKind::ThresholdCrossing { edge } => {
            format!("threshold:{}:{}", edge.u, edge.v)
        }
        KineticEventKind::EdgeOrderSwap { first, second } => {
            format!("order:{}:{}:{}:{}", first.u, first.v, second.u, second.v)
        }
    }
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, threshold=None, dimension=None, modulus=2))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn affine_events(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    threshold: Option<f64>,
    dimension: Option<usize>,
    modulus: u32,
) -> PyResult<(Vec<AffineEventRecord>, Vec<AffineCohomologyRecord>, usize)> {
    py.detach(|| {
        if dimension.is_some() && threshold.is_none() {
            return Err(PyValueError::new_err(
                "dimension requires a fixed threshold",
            ));
        }
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
        let schedule = trajectory.events(threshold).map_err(to_err)?;
        let events = schedule
            .events
            .into_iter()
            .map(|event| {
                (
                    event.time,
                    event.lower,
                    event.upper,
                    event.kinds.iter().map(affine_kind).collect(),
                )
            })
            .collect();
        let relations = match (dimension, threshold) {
            (Some(dimension), Some(scale)) => trajectory
                .cohomology_events(dimension, scale, modulus, CohomologyLimits::default())
                .map_err(to_err)?
                .into_iter()
                .map(|event| {
                    (
                        event.event.time,
                        event.before_rank,
                        event.after_rank,
                        event.relation.relation_rank,
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok((events, relations, schedule.persistent_ties))
    })
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, dimension, scale, modulus=2))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn kinetic_zigzag(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
) -> PyResult<KineticZigzagRecord> {
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
        let limits = KineticZigzagArtifactLimits::default();
        let (artifact, zigzag) =
            KineticZigzagArtifact::build(&trajectory, dimension, scale, modulus, limits)
                .map_err(to_err)?;
        let artifact = artifact.encode(limits).map_err(to_err)?;
        let nodes = zigzag
            .nodes
            .into_iter()
            .map(|node| {
                let (kind, time) = match node.kind {
                    KineticZigzagNodeKind::OpenCell { sample } => ("open", sample),
                    KineticZigzagNodeKind::Event(event) => ("event", event.time),
                };
                (
                    kind.into(),
                    time,
                    node.rank,
                    node.active_edges,
                    node.space.to_string(),
                )
            })
            .collect();
        let arrows = zigzag
            .arrows
            .into_iter()
            .map(|arrow| {
                (
                    match arrow.direction {
                        ZigzagDirection::Forward => "forward",
                        ZigzagDirection::Backward => "backward",
                    }
                    .into(),
                    arrow.restriction.rank,
                )
            })
            .collect();
        let intervals = zigzag
            .barcode
            .intervals
            .into_iter()
            .map(|interval| {
                (
                    interval.id.to_string(),
                    interval.start,
                    interval.end,
                    interval.multiplicity,
                )
            })
            .collect();
        Ok((
            artifact,
            zigzag.barcode.module.to_string(),
            zigzag.persistent_ties,
            nodes,
            arrows,
            intervals,
            zigzag.barcode.generalized_ranks,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, scenarios, candidates, dimension, scale, max_edits, modulus=2, oracle_limit=1_000_000, node_limit=1_000_000))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn intervene_fixed_cohomology(
    py: Python<'_>,
    n: usize,
    scenarios: Vec<CohomologyScenarioInput>,
    candidates: Vec<(usize, usize, u64)>,
    dimension: usize,
    scale: f64,
    max_edits: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CohomologyInterventionRecord> {
    py.detach(|| {
        let scenarios = scenarios
            .into_iter()
            .map(|(triplets, target)| {
                let graph = SparseDistanceMatrix::from_triplets(n, &triplets)?;
                CohomologyInterventionScenario::from_graph(&graph, scale, target)
            })
            .collect::<holos_tda::Result<Vec<_>>>()
            .map_err(to_err)?;
        let mut candidates = candidates
            .into_iter()
            .map(|(u, v, cost)| CohomologyInterventionCandidate::new(u, v, cost))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| candidate.edge);
        let before = candidates.len();
        candidates.dedup_by_key(|candidate| candidate.edge);
        if candidates.len() != before {
            return Err(PyValueError::new_err("candidates repeat an edge"));
        }
        let limits = CohomologyInterventionLimits::default()
            .with_max_oracle_calls(oracle_limit)
            .with_max_search_nodes(node_limit);
        let artifact = CohomologyInterventionArtifact::build(
            n,
            dimension,
            scale,
            modulus,
            &scenarios,
            &candidates,
            max_edits,
            limits,
        )
        .map_err(to_err)?;
        let bytes = artifact.encode(limits).map_err(to_err)?;
        Ok((
            bytes,
            artifact.status().to_string(),
            artifact
                .edits()
                .iter()
                .map(|candidate| (candidate.edge.u, candidate.edge.v, candidate.cost))
                .collect(),
            artifact.lower_bound_cost(),
            artifact.upper_bound_cost(),
            artifact.oracle_calls(),
            artifact.search_nodes(),
            artifact.cache_hits(),
            artifact.root_blockers().to_vec(),
            artifact.before_ranks().to_vec(),
            artifact.after_ranks().to_vec(),
        ))
    })
}
