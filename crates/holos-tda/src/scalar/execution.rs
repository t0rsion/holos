use super::model::{
    CollapseSchedule, Diagram, Engine, Error, GraphFactorization, Result, RipsParams,
};
use super::routing::{graph_routes, may_route, resolved_threshold, square_selected};
use crate::classes::{self, ExplainedDiagram};
use crate::collapse;
use crate::distances;
use crate::distances::{DistanceMatrix, SparseDistanceMatrix};
use crate::factorization;
use crate::solver;

/// Reduce a dense input with the dense engine, from the storage form the
/// rule selects.
fn solve_dense(
    dist: &DistanceMatrix,
    params: &RipsParams,
    threshold: f64,
    edges: Option<usize>,
) -> Result<Diagram> {
    if square_selected(dist, params, threshold, edges) {
        return solver::compute(&dist.to_square(), params);
    }
    solver::compute(dist, params)
}

/// Reduce the thresholded graph of a dense input with the sparse engine.
/// The threshold passes explicitly: a sparse input keeps every listed edge
/// by default, while a dense one stops at the enclosing radius, so an
/// unset threshold here would change the filtration.
fn solve_thresholded(
    dist: &DistanceMatrix,
    params: &RipsParams,
    threshold: f64,
) -> Result<Diagram> {
    let sparse = dist.to_sparse_at(threshold)?;
    let mut inner = params.clone();
    inner.threshold = Some(threshold);
    factorization::compute_sparse(&sparse, &inner)
}

/// Compute the Rips persistence diagram of a distance matrix.
///
/// [`RipsParams::engine`] selects the engine. A run that stays dense then
/// selects its storage form under [`RipsParams::dense_storage`]. A routed
/// run never converts the matrix. See [`crate::Engine`] and [`crate::DenseStorage`].
pub fn rips_persistence(dist: &DistanceMatrix, params: &RipsParams) -> Result<Diagram> {
    if params.collapse_edges {
        return collapse_and_solve(dist, params, |_| Ok(()));
    }
    // The engine resolves the same threshold, so resolving it here and
    // handing it back adds no pass over the matrix.
    let threshold = resolved_threshold(dist, params);
    let mut resolved = params.clone();
    resolved.threshold = Some(threshold);
    match params.engine {
        Engine::Dense => solve_dense(dist, &resolved, threshold, None),
        Engine::Sparse => solve_thresholded(dist, params, threshold),
        Engine::Auto => {
            // The counting pass is the whole cost of a refused route, and
            // the storage rule reads the same count.
            let mut counted = None;
            if may_route(dist.len(), threshold) {
                let edges = dist.count_edges_at(threshold);
                if graph_routes(dist.len(), edges) {
                    return solve_thresholded(dist, &resolved, threshold);
                }
                counted = Some(edges);
            }
            solve_dense(dist, &resolved, threshold, counted)
        }
    }
}

/// Compute a diagram and stable H1 classes from a dense distance matrix.
///
/// The explain path constructs the exact terminal graph, then uses the fixed
/// representative profile described by
/// [`crate::rips_persistence_with_classes_sparse`]. The ordinary compute path keeps
/// its dense and sparse routing choices.
pub fn rips_persistence_with_classes(
    dist: &DistanceMatrix,
    params: &RipsParams,
) -> Result<ExplainedDiagram> {
    let threshold = resolved_threshold(dist, params);
    if params.collapse_edges {
        return dense_collapsed_classes(dist, params);
    }
    let sparse = dist.to_sparse_at(threshold)?;
    let mut fixed = params.clone();
    fixed.threshold = Some(threshold);
    classes::rips_persistence_with_classes_sparse(&sparse, &fixed)
}

fn dense_collapsed_classes(dist: &DistanceMatrix, params: &RipsParams) -> Result<ExplainedDiagram> {
    let collapsed = match params.collapse_schedule {
        CollapseSchedule::Serial => collapse::collapse_dense(dist, params.threshold)?,
        CollapseSchedule::Ordered => {
            collapse::collapse_dense_ordered_parallel(dist, params.threshold, params.threads)?
        }
        CollapseSchedule::Rounds => {
            collapse::collapse_dense_rounds_parallel(dist, params.threshold, params.threads)?
        }
        CollapseSchedule::Adaptive => {
            collapse::collapse_dense_adaptive(dist, params.threshold, params.adaptive_collapse)?
        }
    };
    let mut inner = params.clone();
    inner.collapse_edges = false;
    inner.threshold = Some(collapsed.certificate.terminal_level());
    let explained = classes::rips_persistence_with_classes_sparse(&collapsed.matrix, &inner)?;
    classes::lift_h1_classes(&collapsed, explained)
}

/// Compute the Rips persistence diagram of a sparse distance matrix.
///
/// Pairs not listed in the input are absent at every scale. With no
/// threshold set, all listed edges enter the filtration.
pub fn rips_persistence_sparse(
    dist: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<Diagram> {
    if params.collapse_edges {
        return collapse_and_solve(dist, params, |_| Ok(()));
    }
    factorization::compute_sparse(dist, params)
}

/// The collapse pipeline behind [`rips_persistence`]. One run-wide pool
/// covers the selected collapse and then the reduction. The serial
/// schedule collapses before the pool exists, so the pool goes to the
/// reduction alone. Every surviving edge lies at or below the terminal
/// level, so the terminal level is the exact threshold for the reduced
/// complex. `report` sees the collapse result before the reduction
/// starts.
pub(crate) fn collapse_and_solve<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
    report: impl FnOnce(&collapse::CollapsedRips) -> Result<()>,
) -> Result<Diagram> {
    let (collapsed, pool) = execute_collapse(dist, params)?;
    report(&collapsed)?;
    solve_collapsed(collapsed, pool, params)
}

fn collapse_pool(threads: usize) -> Result<Option<rayon::ThreadPool>> {
    if threads <= 1 {
        return Ok(None);
    }
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map(Some)
        .map_err(|error| Error::Io(format!("thread pool: {error}")))
}

fn execute_collapse<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)> {
    match params.collapse_schedule {
        CollapseSchedule::Serial => {
            collapse_without_pool(dist, params, collapse::collapse_serial_in)
        }
        CollapseSchedule::Ordered => {
            collapse_with_pool(dist, params, collapse::collapse_ordered_in)
        }
        CollapseSchedule::Rounds => collapse_with_pool(dist, params, collapse::collapse_rounds_in),
        CollapseSchedule::Adaptive => collapse_without_pool(dist, params, |dist, threshold| {
            collapse::collapse_adaptive_in(dist, threshold, params.adaptive_collapse)
        }),
    }
}

fn collapse_without_pool<D, F>(
    dist: &D,
    params: &RipsParams,
    collapse: F,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)>
where
    D: distances::Distances,
    F: FnOnce(&D, Option<f64>) -> Result<collapse::CollapsedRips>,
{
    let collapsed = collapse(dist, params.threshold)?;
    Ok((collapsed, collapse_pool(params.threads)?))
}

fn collapse_with_pool<D, F>(
    dist: &D,
    params: &RipsParams,
    collapse: F,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)>
where
    D: distances::Distances + Sync,
    F: FnOnce(&D, Option<f64>, Option<&rayon::ThreadPool>) -> Result<collapse::CollapsedRips>,
{
    let pool = collapse_pool(params.threads)?;
    let collapsed = collapse(dist, params.threshold, pool.as_ref())?;
    Ok((collapsed, pool))
}

fn solve_collapsed(
    collapsed: collapse::CollapsedRips,
    pool: Option<rayon::ThreadPool>,
    params: &RipsParams,
) -> Result<Diagram> {
    let mut inner = params.clone();
    inner.collapse_edges = false;
    inner.threshold = Some(collapsed.certificate.terminal_level());
    if inner.factorization == GraphFactorization::Off {
        solver::compute_in(&collapsed.matrix, &inner, pool)
    } else {
        drop(pool);
        factorization::compute_sparse(&collapsed.matrix, &inner)
    }
}
