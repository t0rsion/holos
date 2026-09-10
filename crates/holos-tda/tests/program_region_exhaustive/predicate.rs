use holos_tda::{
    CertifiedReductionRegion, Diagram, FiltrationSimplex, ReductionGuard, SparseDistanceMatrix,
};

pub(crate) fn full_guard_predicate(
    region: &CertifiedReductionRegion,
    initial: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
) -> bool {
    if initial.len() != updated.len()
        || !initial
            .edges()
            .map(|(u, v, _)| (u, v))
            .eq(updated.edges().map(|(u, v, _)| (u, v)))
    {
        return false;
    }
    let threshold = region.threshold().unwrap_or(f64::INFINITY);
    if initial
        .edges()
        .zip(updated.edges())
        .any(|(left, right)| (left.2 <= threshold) != (right.2 <= threshold))
    {
        return false;
    }
    region
        .complete_guards()
        .iter()
        .all(|guard| guard_holds(updated, guard))
}

fn guard_holds(graph: &SparseDistanceMatrix, guard: &ReductionGuard) -> bool {
    let earlier = simplex_value(graph, guard.earlier());
    let later = simplex_value(graph, guard.later());
    !earlier
        .total_cmp(&later)
        .then_with(|| simplex_rank(guard.later()).cmp(&simplex_rank(guard.earlier())))
        .is_gt()
}

fn simplex_value(graph: &SparseDistanceMatrix, simplex: &FiltrationSimplex) -> f64 {
    match *simplex.vertices() {
        [_] => 0.0,
        [u, v] => graph.get(u, v),
        [u, v, w] => graph.get(u, v).max(graph.get(u, w)).max(graph.get(v, w)),
        _ => panic!("result-sensitive guards only contain vertices, edges, and triangles"),
    }
}

fn simplex_rank(simplex: &FiltrationSimplex) -> u128 {
    match *simplex.vertices() {
        [u] => u as u128,
        [u, v] => v as u128 * v.saturating_sub(1) as u128 / 2 + u as u128,
        [u, v, w] => {
            u as u128
                + v as u128 * v.saturating_sub(1) as u128 / 2
                + w as u128 * w.saturating_sub(1) as u128 * w.saturating_sub(2) as u128 / 6
        }
        _ => panic!("result-sensitive guards only contain vertices, edges, and triangles"),
    }
}

pub(crate) fn diagram_bits_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}
