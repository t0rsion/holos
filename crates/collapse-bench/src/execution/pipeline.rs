use std::time::Instant;

use holos_tda::collapse::{
    CollapsedRips, collapse_dense, collapse_dense_ordered_parallel, collapse_dense_rounds_parallel,
    collapse_sparse, collapse_sparse_ordered_parallel, collapse_sparse_rounds_parallel,
};
use holos_tda::{
    CollapseSchedule, Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix, rips_persistence,
    rips_persistence_sparse,
};

use super::super::model::{Args, Config, Counters, InputKind, Kind, Outcome};

/// One full pipeline run of one configuration, phase by phase. `retain`
/// keeps the collapse result for the ordered gate.
pub(super) fn run_pipeline(
    points: &[Vec<f64>],
    args: &Args,
    cfg: &Config,
    retain: bool,
) -> Result<Outcome, String> {
    let mut phases: Vec<(&'static str, f64)> = Vec::with_capacity(5);
    let whole = Instant::now();
    let dist = timed(&mut phases, "distance", || {
        DistanceMatrix::from_points(points).map_err(|error| error.to_string())
    })?;
    let sparse = collapse_graph(&dist, args, &mut phases)?;
    let graph_edges = sparse.as_ref().map(SparseDistanceMatrix::num_edges);
    if cfg.kind == Kind::V1Product {
        return product_outcome(dist, sparse, args, cfg, phases, whole);
    }
    let collapsed = collapse_phase(&dist, sparse.as_ref(), args, cfg, &mut phases)?;
    let counters = collapsed.as_ref().map(counters_of);
    let mut diagram = timed(&mut phases, "reduce", || {
        reduce_after_collapse(&dist, sparse.as_ref(), collapsed.as_ref(), args)
    })?;
    phases.push(("total", whole.elapsed().as_secs_f64()));
    diagram.canonicalize();

    Ok(Outcome {
        phases,
        diagram,
        counters,
        graph_edges,
        collapsed: if retain { collapsed } else { None },
    })
}

fn timed<T>(
    phases: &mut Vec<(&'static str, f64)>,
    name: &'static str,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let clock = Instant::now();
    let value = operation()?;
    phases.push((name, clock.elapsed().as_secs_f64()));
    Ok(value)
}

fn collapse_graph(
    dist: &DistanceMatrix,
    args: &Args,
    phases: &mut Vec<(&'static str, f64)>,
) -> Result<Option<SparseDistanceMatrix>, String> {
    if args.collapse_input == InputKind::Dense {
        return Ok(None);
    }
    let sparse = timed(phases, "graph", || {
        threshold_to_sparse(dist, args.threshold)
    })?;
    Ok(Some(sparse))
}

fn product_outcome(
    dist: DistanceMatrix,
    sparse: Option<SparseDistanceMatrix>,
    args: &Args,
    cfg: &Config,
    mut phases: Vec<(&'static str, f64)>,
    whole: Instant,
) -> Result<Outcome, String> {
    let params = RipsParams::new(args.max_dim)
        .with_threshold(args.threshold)
        .with_modulus(args.modulus)
        .with_threads(cfg.collapse_threads)
        .with_collapse_schedule(CollapseSchedule::Ordered);
    let mut diagram = timed(&mut phases, "pipeline", || {
        match &sparse {
            Some(matrix) => rips_persistence_sparse(matrix, &params),
            None => rips_persistence(&dist, &params),
        }
        .map_err(|error| error.to_string())
    })?;
    phases.push(("total", whole.elapsed().as_secs_f64()));
    diagram.canonicalize();
    Ok(Outcome {
        phases,
        diagram,
        counters: None,
        graph_edges: sparse.as_ref().map(SparseDistanceMatrix::num_edges),
        collapsed: None,
    })
}

fn collapse_phase(
    dist: &DistanceMatrix,
    sparse: Option<&SparseDistanceMatrix>,
    args: &Args,
    cfg: &Config,
    phases: &mut Vec<(&'static str, f64)>,
) -> Result<Option<CollapsedRips>, String> {
    if cfg.kind == Kind::NoCollapse {
        return Ok(None);
    }
    let result = timed(phases, "collapse", || match sparse {
        Some(matrix) => collapse_sparse_kind(matrix, args, cfg),
        None => collapse_dense_kind(dist, args, cfg),
    })?;
    Ok(Some(result))
}

fn collapse_sparse_kind(
    sparse: &SparseDistanceMatrix,
    args: &Args,
    cfg: &Config,
) -> Result<CollapsedRips, String> {
    let threshold = Some(args.threshold);
    let result = match cfg.kind {
        Kind::V2 => collapse_sparse_rounds_parallel(sparse, threshold, cfg.collapse_threads),
        Kind::V1 => collapse_sparse(sparse, threshold),
        Kind::V1Ordered => {
            collapse_sparse_ordered_parallel(sparse, threshold, cfg.collapse_threads)
        }
        Kind::V1Product | Kind::NoCollapse => unreachable!("both take an earlier branch"),
    };
    result.map_err(|error| error.to_string())
}

fn collapse_dense_kind(
    dist: &DistanceMatrix,
    args: &Args,
    cfg: &Config,
) -> Result<CollapsedRips, String> {
    let threshold = Some(args.threshold);
    let result = match cfg.kind {
        Kind::V2 => collapse_dense_rounds_parallel(dist, threshold, cfg.collapse_threads),
        Kind::V1 => collapse_dense(dist, threshold),
        Kind::V1Ordered => collapse_dense_ordered_parallel(dist, threshold, cfg.collapse_threads),
        Kind::V1Product | Kind::NoCollapse => unreachable!("both take an earlier branch"),
    };
    result.map_err(|error| error.to_string())
}

fn reduce_after_collapse(
    dist: &DistanceMatrix,
    sparse: Option<&SparseDistanceMatrix>,
    collapsed: Option<&CollapsedRips>,
    args: &Args,
) -> Result<Diagram, String> {
    let result = match (collapsed, sparse) {
        (Some(result), _) => {
            let params = reduce_params(args, result.certificate.terminal_level());
            rips_persistence_sparse(&result.matrix, &params)
        }
        (None, Some(matrix)) => {
            rips_persistence_sparse(matrix, &reduce_params(args, args.threshold))
        }
        (None, None) => rips_persistence(dist, &reduce_params(args, args.threshold)),
    };
    result.map_err(|error| error.to_string())
}

fn reduce_params(args: &Args, threshold: f64) -> RipsParams {
    RipsParams::new(args.max_dim)
        .with_threshold(threshold)
        .with_modulus(args.modulus)
        .with_threads(args.reducer_threads)
}

fn counters_of(collapsed: &CollapsedRips) -> Counters {
    let stats = &collapsed.stats;
    let mut batch_widths: Vec<(usize, usize)> = Vec::new();
    for step in collapsed.certificate.steps() {
        let epoch = step.position().number();
        match batch_widths.last_mut() {
            Some(last) if last.0 == epoch => last.1 += 1,
            _ => batch_widths.push((epoch, 1)),
        }
    }
    Counters {
        algorithm_version: collapsed.certificate.algorithm_version(),
        terminal_level: collapsed.certificate.terminal_level(),
        input_edges: stats.input_edges,
        output_edges: stats.output_edges,
        removed_edges: stats.removed_edges,
        epochs: stats.epochs,
        edge_tests: stats.edge_tests,
        logical_tests: stats.logical_tests,
        invalidated_results: stats.invalidated_results,
        global_invalidations: stats.global_invalidations,
        window_batches: stats.window_batches,
        window_slots_offered: stats.window_slots_offered,
        window_members_formed: stats.window_members_formed,
        window_members_reused: stats.window_members_reused,
        witness_segments: stats.witness_segments,
        max_common_neighborhood: stats.max_common_neighborhood,
        predicate_ns: collapsed.timings.predicate_ns,
        retirement_ns: collapsed.timings.retirement_ns,
        repair_ns: collapsed.timings.repair_ns,
        batch_widths,
    }
}

/// The thresholded graph of a dense matrix, as the collapse and the
/// no-collapse baseline both see it.
fn threshold_to_sparse(
    dist: &DistanceMatrix,
    threshold: f64,
) -> Result<SparseDistanceMatrix, String> {
    let n = dist.len();
    let mut triplets: Vec<(usize, usize, f64)> = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = dist.get(i, j);
            if d.is_finite() && d <= threshold {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).map_err(|e| e.to_string())
}
