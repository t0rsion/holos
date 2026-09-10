use rustc_hash::FxHashMap;

use crate::collapse::{CollapsedRips, verify::verify_sparse};
use crate::{Diagram, Error, GraphFactorization, Result, RipsParams, SparseDistanceMatrix};

use super::canonical::{canonical_edge, canonical_spaces, normalize_map, recanonicalize_space};
use super::model::{ExplainedDiagram, PersistentClass, PersistentClassSpace};
use super::validation::{oriented_coefficient, validate_h1_cocycle};

/// Compute a diagram and stable H1 classes from a sparse matrix.
///
/// On a fixed graph, class production uses a fixed whole-graph reduction
/// profile. Class identifiers are independent of worker count, structural
/// routing, and optional reduction shortcuts.
pub fn rips_persistence_with_classes_sparse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<ExplainedDiagram> {
    check_class_params(params)?;
    if params.collapse_edges {
        return collapsed_classes(matrix, params);
    }
    let fixed = fixed_class_params(params);
    let (diagram, raw) = crate::solver::compute_with_h1_classes(matrix, &fixed)?;
    let spaces = canonical_spaces(matrix, params.modulus, raw)?;
    check_class_count(&diagram, &spaces)?;
    Ok(ExplainedDiagram { diagram, spaces })
}

fn check_class_params(params: &RipsParams) -> Result<()> {
    if params.max_dim < 1 {
        return Err(Error::InvalidInput(
            "H1 classes require max_dim of at least 1".into(),
        ));
    }
    Ok(())
}

fn collapsed_classes(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<ExplainedDiagram> {
    let collapsed = match params.collapse_schedule {
        crate::CollapseSchedule::Serial => {
            crate::collapse::collapse_sparse(matrix, params.threshold)?
        }
        crate::CollapseSchedule::Ordered => crate::collapse::collapse_sparse_ordered_parallel(
            matrix,
            params.threshold,
            params.threads,
        )?,
        crate::CollapseSchedule::Rounds => crate::collapse::collapse_sparse_rounds_parallel(
            matrix,
            params.threshold,
            params.threads,
        )?,
        crate::CollapseSchedule::Adaptive => crate::collapse::collapse_sparse_adaptive(
            matrix,
            params.threshold,
            params.adaptive_collapse,
        )?,
    };
    let mut inner = params.clone();
    inner.collapse_edges = false;
    inner.threshold = Some(collapsed.certificate.terminal_level());
    let explained = rips_persistence_with_classes_sparse(&collapsed.matrix, &inner)?;
    lift_h1_classes(&collapsed, explained)
}

fn fixed_class_params(params: &RipsParams) -> RipsParams {
    let mut fixed = params.clone();
    fixed.factorization = GraphFactorization::Off;
    fixed.use_emergent_pairs = false;
    fixed.use_apparent_pairs = false;
    fixed.use_adjacency_rows = false;
    fixed.use_clearing = true;
    fixed
}

fn check_class_count(diagram: &Diagram, spaces: &[PersistentClassSpace]) -> Result<()> {
    let h1 = diagram.in_dim(1).count();
    let class_count: usize = spaces.iter().map(|space| space.basis.len()).sum();
    if h1 != class_count {
        return Err(Error::InvalidInput(format!(
            "H1 reduction returned {h1} intervals but {class_count} basis classes"
        )));
    }
    Ok(())
}

/// Lift stable H1 cocycles through a checked collapse trace.
///
/// The input classes must describe `collapsed.matrix`. The returned classes
/// use the reconstructed input vertex labels and edges. The collapse
/// certificate and every lifted cocycle are checked on the original graph.
pub fn lift_h1_classes(
    collapsed: &CollapsedRips,
    mut explained: ExplainedDiagram,
) -> Result<ExplainedDiagram> {
    let original = reconstruct_input(collapsed)?;
    verify_sparse(
        &original,
        collapsed.certificate.requested_threshold(),
        collapsed,
    )
    .map_err(|error| Error::InvalidInput(format!("collapse lift: {error}")))?;
    for space in &mut explained.spaces {
        for class in &mut space.basis {
            lift_one(collapsed, class)?;
            validate_h1_cocycle(&original, &class.cocycle)?;
        }
        recanonicalize_space(&original, space)?;
    }
    explained.spaces.sort_by(|a, b| {
        a.interval
            .birth
            .total_cmp(&b.interval.birth)
            .then(a.interval.death.total_cmp(&b.interval.death))
            .then(a.id.cmp(&b.id))
    });
    Ok(explained)
}

fn reconstruct_input(collapsed: &CollapsedRips) -> Result<SparseDistanceMatrix> {
    let mut triplets: Vec<_> = collapsed.matrix.edges().collect();
    triplets.extend(collapsed.certificate.steps().iter().map(|step| {
        let (u, v) = step.edge();
        (u, v, step.value())
    }));
    SparseDistanceMatrix::from_triplets(collapsed.certificate.vertex_count(), &triplets)
}

fn lift_one(collapsed: &CollapsedRips, class: &mut PersistentClass) -> Result<()> {
    let modulus = class.cocycle.modulus as u64;
    let scale = class.cocycle.scale;
    let mut live: FxHashMap<(usize, usize), f64> = collapsed
        .matrix
        .edges()
        .map(|(u, v, value)| ((u, v), value))
        .collect();
    let mut coefficients: FxHashMap<(usize, usize), u64> = class
        .cocycle
        .terms
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient as u64))
        .collect();

    for step in collapsed.certificate.steps().iter().rev() {
        let (u, v) = step.edge();
        if step.value() <= scale {
            let witness = step
                .witnesses()
                .iter()
                .rev()
                .find(|&&(start, _)| start <= scale)
                .map(|&(_, witness)| witness)
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "collapse lift: edge ({u}, {v}) has no witness at scale {scale}"
                    ))
                })?;
            for (a, b) in [(u, witness), (v, witness)] {
                let edge = canonical_edge(a, b);
                let Some(&value) = live.get(&edge) else {
                    return Err(Error::InvalidInput(format!(
                        "collapse lift: witness edge ({}, {}) is not live",
                        edge.0, edge.1
                    )));
                };
                if value > scale {
                    return Err(Error::InvalidInput(format!(
                        "collapse lift: witness edge ({}, {}) is born after scale {scale}",
                        edge.0, edge.1
                    )));
                }
            }
            let vw = oriented_coefficient(&coefficients, v, witness, modulus);
            let wu = oriented_coefficient(&coefficients, witness, u, modulus);
            let coefficient = (modulus - (vw + wu) % modulus) % modulus;
            if coefficient != 0 {
                coefficients.insert((u, v), coefficient);
            }
        }
        if live.insert((u, v), step.value()).is_some() {
            return Err(Error::InvalidInput(format!(
                "collapse lift: edge ({u}, {v}) is restored twice"
            )));
        }
    }
    class.cocycle.terms = normalize_map(coefficients, modulus)?;
    Ok(())
}
