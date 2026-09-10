use crate::distances::Distances;
use crate::{Error, Result, SparseDistanceMatrix};

use super::model::{
    CollapseCertificate, CollapseCompleteness, CollapseObjective, CollapseStats, CollapseTimings,
    CollapsedRips, RemovalStep,
};

/// One edge of the thresholded input, held in schedule order. Removal
/// clears `alive` and tombstones the two adjacency entries.
pub(super) struct EdgeRec {
    pub(super) u: usize,
    pub(super) v: usize,
    pub(super) value: f64,
    pub(super) alive: bool,
}

/// An adjacency entry: neighbor, current value (+inf once tombstoned), and
/// the position of the edge in the schedule array.
pub(super) type AdjEntry = (usize, f64, usize);

/// The certificate header of a run: vertex count, terminal level, and
/// the threshold the caller passed.
pub(super) struct Run {
    pub(super) n: usize,
    pub(super) terminal: f64,
    pub(super) threshold: Option<f64>,
}

/// The thresholded input of one collapse.
pub(super) struct Prepared {
    /// The edges in schedule order.
    pub(super) edges: Vec<EdgeRec>,
    /// Adjacency lists over `edges`, each in ascending neighbor order.
    pub(super) adj: Vec<Vec<AdjEntry>>,
    /// What the certificate reports besides the edges.
    pub(super) run: Run,
}

/// Which execution produced a run.
///
/// It sets the certificate version and the adaptive stopping fields.
#[derive(Clone, Copy)]
pub(super) enum Execution {
    /// The serial version 1 run: certificate version 1, and the physical
    /// test sequence is the logical one.
    Serial,
    /// The ordered speculative version 1 run: certificate version 1, and
    /// the execution counts its own logical trace, which the physical
    /// count can exceed.
    Ordered,
    /// The version 2 rounds schedule: certificate version 2, and the
    /// physical test sequence is the logical one.
    Snapshot,
    /// The adaptive version 3 schedule and its declared stopping state.
    Adaptive {
        objective: CollapseObjective,
        completeness: CollapseCompleteness,
        work_limit: Option<u64>,
        work_used: u64,
    },
}

/// Collect the thresholded input and index it. Every execution starts
/// here, so they see the same edge order and the same terminal level.
pub(super) fn prepare<D: Distances>(dist: &D, threshold: Option<f64>) -> Result<Prepared> {
    validate_threshold(threshold)?;
    let n = dist.len();
    let resolved = threshold.unwrap_or_else(|| dist.default_threshold());

    let mut edges: Vec<EdgeRec> = Vec::new();
    dist.for_each_edge(|i, j, d| {
        if d.is_finite() && d <= resolved {
            edges.push(EdgeRec {
                u: j,
                v: i,
                value: d,
                alive: true,
            });
        }
    });
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        edges.iter().map(|e| e.value).fold(0.0f64, f64::max)
    };

    edges.sort_unstable_by(|a, b| {
        b.value
            .total_cmp(&a.value)
            // The combinadic edge index v*(v-1)/2 + u orders exactly like
            // (v, u) for u < v, and the field compare cannot overflow.
            .then_with(|| (a.v, a.u).cmp(&(b.v, b.u)))
    });

    let mut adj: Vec<Vec<AdjEntry>> = vec![Vec::new(); n];
    for (idx, e) in edges.iter().enumerate() {
        adj[e.u].push((e.v, e.value, idx));
        adj[e.v].push((e.u, e.value, idx));
    }
    for list in &mut adj {
        list.sort_unstable_by_key(|&(x, _, _)| x);
    }

    Ok(Prepared {
        edges,
        adj,
        run: Run {
            n,
            terminal,
            threshold,
        },
    })
}

/// Close a run: keep the live edges, build the reduced matrix, and record
/// the removals in a certificate. `stats` holds the counters the execution
/// kept, and this step adds the totals that follow from `steps`.
pub(super) fn finish(
    run: Run,
    execution: Execution,
    edges: &[EdgeRec],
    steps: Vec<RemovalStep>,
    mut stats: CollapseStats,
    timings: CollapseTimings,
) -> Result<CollapsedRips> {
    let input_edges = edges.len();
    stats.removed_edges = steps.len();
    stats.output_edges = input_edges - steps.len();
    if !matches!(execution, Execution::Ordered) {
        // The serial and rounds executions test exactly the logical
        // sequence, so the physical count is the logical count. The
        // ordered execution counts its logical trace as it retires.
        stats.logical_tests = stats.edge_tests;
    }

    let survivors: Vec<(usize, usize, f64)> = edges
        .iter()
        .filter(|e| e.alive)
        .map(|e| (e.u, e.v, e.value))
        .collect();
    let matrix = SparseDistanceMatrix::from_triplets(run.n, &survivors)?;
    let (algorithm_version, objective, completeness, work_limit, work_used) = match execution {
        Execution::Serial | Execution::Ordered => {
            (1, None, CollapseCompleteness::CompleteFixedPoint, None, 0)
        }
        Execution::Snapshot => (2, None, CollapseCompleteness::CompleteFixedPoint, None, 0),
        Execution::Adaptive {
            objective,
            completeness,
            work_limit,
            work_used,
        } => (3, Some(objective), completeness, work_limit, work_used),
    };
    let certificate = CollapseCertificate {
        algorithm_version,
        objective,
        completeness,
        work_limit,
        work_used,
        vertex_count: run.n,
        requested_threshold: run.threshold,
        terminal_level: run.terminal,
        input_edge_count: input_edges,
        output_edge_count: stats.output_edges,
        steps,
    };
    Ok(CollapsedRips {
        matrix,
        certificate,
        stats,
        timings,
    })
}

/// A rayon pool with `threads` workers, owned by one call.
pub(super) fn build_pool(threads: usize) -> Result<rayon::ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| Error::Io(format!("thread pool: {e}")))
}

/// Tombstone both adjacency entries of a removed edge. The entries stay in
/// place so binary search and the sorted merge keep working.
pub(super) fn tombstone(adj: &mut [Vec<AdjEntry>], u: usize, v: usize) {
    for (a, b) in [(u, v), (v, u)] {
        if let Ok(pos) = adj[a].binary_search_by(|probe| probe.0.cmp(&b)) {
            adj[a][pos].1 = f64::INFINITY;
        }
    }
}

/// Shared threshold validation, matching the solver's rule.
fn validate_threshold(threshold: Option<f64>) -> Result<()> {
    if let Some(t) = threshold {
        if t.is_nan() || t < 0.0 {
            return Err(Error::InvalidInput(format!(
                "threshold must be non-negative, got {t}"
            )));
        }
    }
    Ok(())
}
