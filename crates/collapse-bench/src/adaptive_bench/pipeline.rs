use std::time::Instant;

use holos_tda::collapse::verify::verify_sparse_artifact;
use holos_tda::collapse::wire::{CollapseArtifact, DecodeLimits};
use holos_tda::collapse::{
    AdaptiveCollapseParams, CollapseCompleteness, CollapseObjective, CollapsedRips,
    collapse_sparse, collapse_sparse_adaptive, collapse_sparse_rounds_parallel,
};
use holos_tda::{
    Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};

use super::args::{Args, Kind};
use super::input::{graph_cliques, threshold_to_sparse};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) algorithm_version: u32,
    pub(super) completeness: &'static str,
    pub(super) input_edges: usize,
    pub(super) output_edges: usize,
    pub(super) removed_edges: usize,
    pub(super) steps: usize,
    pub(super) edge_tests: usize,
    pub(super) scored: usize,
    pub(super) queue_pops: usize,
    pub(super) stale_pops: usize,
    pub(super) triangles_removed: u64,
    pub(super) tetrahedra_removed: u64,
    pub(super) witness_segments: usize,
    pub(super) work_used: u64,
    pub(super) artifact_bytes: usize,
    pub(super) output_triangles: u64,
    pub(super) output_tetrahedra: u64,
}

pub(super) struct Sample {
    pub(super) distance_s: f64,
    pub(super) graph_s: f64,
    pub(super) collapse_s: f64,
    pub(super) reduce_s: f64,
    pub(super) compute_s: f64,
    pub(super) artifact_s: f64,
    pub(super) verify_s: f64,
    pub(super) certified_s: f64,
    pub(super) counts: Option<Counts>,
}

pub(super) struct Outcome {
    pub(super) sample: Sample,
    pub(super) diagram: Diagram,
}

struct Certification {
    artifact_s: f64,
    verify_s: f64,
    certified_s: f64,
    counts: Option<Counts>,
}

pub(super) fn agreement_gate(points: &[Vec<f64>], args: &Args) -> Result<Vec<Diagram>, String> {
    let reference = run_one(points, args, Kind::None)?.diagram;
    let mut references: Vec<Diagram> = Vec::with_capacity(args.kinds.len());
    for &kind in &args.kinds {
        let outcome = run_one(points, args, kind)?;
        if !diagrams_equal(&reference, &outcome.diagram) {
            return Err(format!(
                "{} disagrees with no collapse bar for bar; timings are void",
                kind.name()
            ));
        }
        references.push(outcome.diagram);
    }
    Ok(references)
}

pub(super) fn run_one(points: &[Vec<f64>], args: &Args, kind: Kind) -> Result<Outcome, String> {
    let whole = Instant::now();
    let phase = Instant::now();
    let dense = DistanceMatrix::from_points(points).map_err(|error| error.to_string())?;
    let distance_s = phase.elapsed().as_secs_f64();

    let phase = Instant::now();
    let sparse = threshold_to_sparse(&dense, args.threshold)?;
    let graph_s = phase.elapsed().as_secs_f64();

    let (collapsed, collapse_s) = run_collapse(&sparse, args, kind)?;

    let (mut diagram, reduce_s) = run_reduction(&sparse, collapsed.as_ref(), args)?;
    let compute_s = whole.elapsed().as_secs_f64();
    let certification = certify_run(&sparse, collapsed.as_ref(), args.threshold, &whole)?;
    diagram.canonicalize();
    Ok(Outcome {
        sample: Sample {
            distance_s,
            graph_s,
            collapse_s,
            reduce_s,
            compute_s,
            artifact_s: certification.artifact_s,
            verify_s: certification.verify_s,
            certified_s: certification.certified_s,
            counts: certification.counts,
        },
        diagram,
    })
}

fn run_collapse(
    graph: &SparseDistanceMatrix,
    args: &Args,
    kind: Kind,
) -> Result<(Option<CollapsedRips>, f64), String> {
    if kind == Kind::None {
        return Ok((None, 0.0));
    }
    let started = Instant::now();
    let result = collapse_for_kind(graph, args, kind).map_err(|error| error.to_string())?;
    Ok((Some(result), started.elapsed().as_secs_f64()))
}

fn collapse_for_kind(
    graph: &SparseDistanceMatrix,
    args: &Args,
    kind: Kind,
) -> holos_tda::Result<CollapsedRips> {
    match kind {
        Kind::V1 => collapse_sparse(graph, Some(args.threshold)),
        Kind::V2 => collapse_sparse_rounds_parallel(graph, Some(args.threshold), args.threads),
        Kind::V3H1 => run_adaptive(graph, args, CollapseObjective::H1),
        Kind::V3H2 => run_adaptive(graph, args, CollapseObjective::H2),
        Kind::None => unreachable!(),
    }
}

fn run_adaptive(
    graph: &SparseDistanceMatrix,
    args: &Args,
    objective: CollapseObjective,
) -> holos_tda::Result<CollapsedRips> {
    let mut params = AdaptiveCollapseParams::new(objective);
    params.work_limit = args.work_limit;
    collapse_sparse_adaptive(graph, Some(args.threshold), params)
}

fn run_reduction(
    graph: &SparseDistanceMatrix,
    collapsed: Option<&CollapsedRips>,
    args: &Args,
) -> Result<(Diagram, f64), String> {
    let started = Instant::now();
    let reduce_input = collapsed.map_or(graph, |result| &result.matrix);
    let threshold = collapsed.map_or(args.threshold, |result| result.certificate.terminal_level());
    let params = RipsParams::new(args.max_dim)
        .with_threshold(threshold)
        .with_modulus(args.modulus)
        .with_threads(args.threads);
    let diagram =
        rips_persistence_sparse(reduce_input, &params).map_err(|error| error.to_string())?;
    Ok((diagram, started.elapsed().as_secs_f64()))
}

fn certify_run(
    graph: &SparseDistanceMatrix,
    collapsed: Option<&CollapsedRips>,
    threshold: f64,
    whole: &Instant,
) -> Result<Certification, String> {
    let Some(result) = collapsed else {
        return Ok(Certification {
            artifact_s: 0.0,
            verify_s: 0.0,
            certified_s: whole.elapsed().as_secs_f64(),
            counts: None,
        });
    };
    let artifact_started = Instant::now();
    let bytes = CollapseArtifact::from_result(result)
        .and_then(|artifact| artifact.encode())
        .map_err(|error| error.to_string())?;
    let artifact_s = artifact_started.elapsed().as_secs_f64();
    let verify_started = Instant::now();
    let artifact = CollapseArtifact::decode(&bytes, DecodeLimits::default())
        .map_err(|error| error.to_string())?;
    verify_sparse_artifact(graph, Some(threshold), &artifact).map_err(|error| error.to_string())?;
    Ok(Certification {
        artifact_s,
        verify_s: verify_started.elapsed().as_secs_f64(),
        certified_s: whole.elapsed().as_secs_f64(),
        counts: Some(counts(result, bytes.len())),
    })
}

fn counts(result: &CollapsedRips, artifact_bytes: usize) -> Counts {
    let completeness = match result.certificate.completeness() {
        CollapseCompleteness::CompleteFixedPoint => "complete",
        CollapseCompleteness::BudgetLimited => "budget_limited",
        _ => "unknown",
    };
    let (output_triangles, output_tetrahedra) = graph_cliques(&result.matrix);
    Counts {
        algorithm_version: result.certificate.algorithm_version(),
        completeness,
        input_edges: result.stats.input_edges,
        output_edges: result.stats.output_edges,
        removed_edges: result.stats.removed_edges,
        steps: result.certificate.steps().len(),
        edge_tests: result.stats.edge_tests,
        scored: result.stats.adaptive_score_evaluations,
        queue_pops: result.stats.adaptive_queue_pops,
        stale_pops: result.stats.adaptive_stale_pops,
        triangles_removed: result.stats.adaptive_triangles_removed,
        tetrahedra_removed: result.stats.adaptive_tetrahedra_removed,
        witness_segments: result.stats.witness_segments,
        work_used: result.certificate.work_used(),
        artifact_bytes,
        output_triangles,
        output_tetrahedra,
    }
}

pub(super) fn diagrams_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(x, y)| {
            x.dim == y.dim
                && x.birth.to_bits() == y.birth.to_bits()
                && x.death.to_bits() == y.death.to_bits()
        })
}
