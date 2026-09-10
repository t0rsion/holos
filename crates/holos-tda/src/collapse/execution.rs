use crate::distances::Distances;
use crate::{DistanceMatrix, Result, SparseDistanceMatrix};

use super::domination::{Scratch, mark_dirty, test_edge};
use super::model::{CollapseStats, CollapseTimings, CollapsedRips, RemovalStep, SchedulePosition};
use super::preparation::{Execution, Prepared, finish, prepare, tombstone};

/// Collapse a dense distance matrix with the serial schedule.
///
/// `threshold` follows the engine's rule: `None` means the enclosing
/// radius. Edges above the resolved threshold are dropped before the
/// collapse. The parallel forms are
/// [`crate::collapse::collapse_dense_ordered_parallel`] and
/// [`crate::collapse::collapse_dense_rounds_parallel`].
pub fn collapse_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

/// Collapse a sparse distance matrix with the serial schedule.
///
/// `threshold` follows the engine's rule: `None` keeps every listed edge.
/// The parallel forms are [`crate::collapse::collapse_sparse_ordered_parallel`]
/// and [`crate::collapse::collapse_sparse_rounds_parallel`].
pub fn collapse_sparse(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

/// The serial version 1 collapse for the pipeline.
pub(crate) fn collapse_serial_in<D: Distances>(
    dist: &D,
    threshold: Option<f64>,
) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

pub(super) fn collapse_impl<D: Distances>(
    dist: &D,
    threshold: Option<f64>,
) -> Result<CollapsedRips> {
    let Prepared {
        mut edges,
        mut adj,
        run,
    } = prepare(dist, threshold)?;

    let mut stats = CollapseStats::new(edges.len());
    let mut steps: Vec<RemovalStep> = Vec::new();
    let mut scratch = Scratch::default();
    // A failed verdict can change only after an edge in its neighborhood
    // is removed. Later passes retest dirty edges, or every live edge
    // after a MARK_LIMIT bail. Both sets cover every edge whose verdict
    // could have changed, so the certificate matches a full retest.
    let mut dirty: Vec<bool> = vec![false; edges.len()];
    let mut test_all = true;
    loop {
        stats.epochs += 1;
        let mut removed_any = false;
        let mut test_all_next = false;
        for idx in 0..edges.len() {
            if !edges[idx].alive || !(test_all || dirty[idx]) {
                continue;
            }
            dirty[idx] = false;
            let (u, v, value) = (edges[idx].u, edges[idx].v, edges[idx].value);
            stats.edge_tests += 1;
            let witnesses = test_edge(&adj, u, v, value, run.terminal, &mut scratch);
            stats.max_common_neighborhood = stats.max_common_neighborhood.max(scratch.cands.len());
            if let Some(witnesses) = witnesses {
                edges[idx].alive = false;
                if !mark_dirty(&adj, &mut dirty, u, v, &mut scratch) {
                    test_all_next = true;
                }
                tombstone(&mut adj, u, v);
                stats.witness_segments += witnesses.len();
                steps.push(RemovalStep {
                    u,
                    v,
                    value,
                    position: SchedulePosition::Pass(stats.epochs),
                    witnesses,
                });
                removed_any = true;
            }
        }
        if !removed_any {
            break;
        }
        test_all = test_all_next;
    }

    finish(
        run,
        Execution::Serial,
        &edges,
        steps,
        stats,
        CollapseTimings::default(),
    )
}
