use crate::collapse::{
    CollapseCertificate, CollapseCompleteness, CollapseStats, CollapsedRips, RemovalStep,
    SchedulePosition,
};
use crate::{DistanceMatrix, SparseDistanceMatrix};

pub(crate) fn triangle_dense() -> DistanceMatrix {
    DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap()
}

pub(crate) fn triangle_sparse() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0)]).unwrap()
}

pub(crate) fn stats_for(input: usize, output: usize) -> CollapseStats {
    CollapseStats {
        input_edges: input,
        output_edges: output,
        removed_edges: input - output,
        epochs: 1,
        edge_tests: 0,
        witness_segments: 0,
        max_common_neighborhood: 0,
        window_slots_offered: 0,
        window_members_formed: 0,
        window_members_reused: 0,
        logical_tests: 0,
        invalidated_results: 0,
        global_invalidations: 0,
        window_batches: 0,
        adaptive_score_evaluations: 0,
        adaptive_queue_pops: 0,
        adaptive_stale_pops: 0,
        adaptive_triangles_removed: 0,
        adaptive_tetrahedra_removed: 0,
    }
}

/// Hand-built result for the unit triangle: edge (0, 1) removed with
/// the single witness segment (1.0, apex 2); the path 0-2, 1-2 remains.
pub(crate) fn triangle_result() -> CollapsedRips {
    CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(3, &[(0, 2, 1.0), (1, 2, 1.0)]).unwrap(),
        certificate: CollapseCertificate {
            algorithm_version: 1,
            objective: None,
            completeness: CollapseCompleteness::CompleteFixedPoint,
            work_limit: None,
            work_used: 0,
            vertex_count: 3,
            requested_threshold: None,
            terminal_level: 1.0,
            input_edge_count: 3,
            output_edge_count: 2,
            steps: vec![RemovalStep {
                u: 0,
                v: 1,
                value: 1.0,
                position: SchedulePosition::Pass(1),
                witnesses: vec![(1.0, 2)],
            }],
        },
        stats: stats_for(3, 2),
        timings: Default::default(),
    }
}

/// Same removal as [`triangle_result`], but at requested threshold 2.0
/// so the terminal level sits above the only critical value 1.0.
pub(crate) fn triangle_result_threshold_two() -> CollapsedRips {
    let mut result = triangle_result();
    result.certificate.requested_threshold = Some(2.0);
    result.certificate.terminal_level = 2.0;
    result
}

pub(crate) fn k4_dense() -> DistanceMatrix {
    DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap()
}

/// Hand-built result for the unit K4: pass 1 removes (0, 1), (0, 2),
/// and (1, 2) in schedule order; the star at vertex 3 remains.
pub(crate) fn k4_result() -> CollapsedRips {
    CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(4, &[(0, 3, 1.0), (1, 3, 1.0), (2, 3, 1.0)])
            .unwrap(),
        certificate: CollapseCertificate {
            algorithm_version: 1,
            objective: None,
            completeness: CollapseCompleteness::CompleteFixedPoint,
            work_limit: None,
            work_used: 0,
            vertex_count: 4,
            requested_threshold: None,
            terminal_level: 1.0,
            input_edge_count: 6,
            output_edge_count: 3,
            steps: vec![
                RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    position: SchedulePosition::Pass(1),
                    witnesses: vec![(1.0, 2)],
                },
                RemovalStep {
                    u: 0,
                    v: 2,
                    value: 1.0,
                    position: SchedulePosition::Pass(1),
                    witnesses: vec![(1.0, 3)],
                },
                RemovalStep {
                    u: 1,
                    v: 2,
                    value: 1.0,
                    position: SchedulePosition::Pass(1),
                    witnesses: vec![(1.0, 3)],
                },
            ],
        },
        stats: stats_for(6, 3),
        timings: Default::default(),
    }
}

/// Result with a two-level candidate set for edge (0, 1): vertex 2
/// enters at 1.0, vertex 3 at 2.0, and f(2, 3) = 2.0 keeps vertex 2
/// dominating through the terminal level 2.0. The single recorded step
/// is valid, but the graph is not a fixed point afterward; use this
/// fixture only for rejections that fire inside step 0.
pub(crate) fn two_level_dense() -> DistanceMatrix {
    DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0]).unwrap()
}

pub(crate) fn two_level_result() -> CollapsedRips {
    CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 2, 1.0),
                (0, 3, 2.0),
                (1, 2, 1.0),
                (1, 3, 2.0),
                (2, 3, 2.0),
            ],
        )
        .unwrap(),
        certificate: CollapseCertificate {
            algorithm_version: 1,
            objective: None,
            completeness: CollapseCompleteness::CompleteFixedPoint,
            work_limit: None,
            work_used: 0,
            vertex_count: 4,
            requested_threshold: Some(2.0),
            terminal_level: 2.0,
            input_edge_count: 6,
            output_edge_count: 5,
            steps: vec![RemovalStep {
                u: 0,
                v: 1,
                value: 1.0,
                position: SchedulePosition::Pass(1),
                witnesses: vec![(1.0, 2)],
            }],
        },
        stats: stats_for(6, 5),
        timings: Default::default(),
    }
}

pub(crate) fn two_k4_dense() -> DistanceMatrix {
    let mut cond = Vec::new();
    for v in 1..8usize {
        for u in 0..v {
            cond.push(if (u < 4) == (v < 4) {
                1.0
            } else {
                f64::INFINITY
            });
        }
    }
    DistanceMatrix::from_condensed(cond).unwrap()
}

pub(crate) fn v2_step(u: usize, v: usize, round: usize, apex: usize) -> RemovalStep {
    RemovalStep {
        u,
        v,
        value: 1.0,
        position: SchedulePosition::Round(round),
        witnesses: vec![(1.0, apex)],
    }
}

/// Hand-derived version 2 result for the two disjoint K4s. Round 1
/// removes (0, 1) and (4, 5); each blocks every other edge of its own
/// component. Round 2 removes (0, 2), (1, 2), (4, 6), (5, 6): in each
/// round 2 snapshot the surviving hub (3 or 7) is the only candidate,
/// and the two selected edges do not conflict. The stars at vertices
/// 3 and 7 remain and no further edge is removable.
pub(crate) fn two_k4_v2_result() -> CollapsedRips {
    CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(
            8,
            &[
                (0, 3, 1.0),
                (1, 3, 1.0),
                (2, 3, 1.0),
                (4, 7, 1.0),
                (5, 7, 1.0),
                (6, 7, 1.0),
            ],
        )
        .unwrap(),
        certificate: CollapseCertificate {
            algorithm_version: 2,
            objective: None,
            completeness: CollapseCompleteness::CompleteFixedPoint,
            work_limit: None,
            work_used: 0,
            vertex_count: 8,
            requested_threshold: None,
            terminal_level: 1.0,
            input_edge_count: 12,
            output_edge_count: 6,
            steps: vec![
                v2_step(0, 1, 1, 2),
                v2_step(4, 5, 1, 6),
                v2_step(0, 2, 2, 3),
                v2_step(1, 2, 2, 3),
                v2_step(4, 6, 2, 7),
                v2_step(5, 6, 2, 7),
            ],
        },
        stats: stats_for(12, 6),
        timings: Default::default(),
    }
}

/// Version 2 result for the unit K4 with one recorded round; callers
/// pick the steps. The output matrix is the K4 minus the removed
/// edges, so the header checks pass and the replay reaches the round.
pub(crate) fn k4_v2_round(steps: Vec<RemovalStep>) -> CollapsedRips {
    let gone: Vec<(usize, usize)> = steps.iter().map(|s| (s.u, s.v)).collect();
    let survivors: Vec<(usize, usize, f64)> = (0..4usize)
        .flat_map(|u| (u + 1..4).map(move |v| (u, v, 1.0)))
        .filter(|&(u, v, _)| !gone.contains(&(u, v)))
        .collect();
    CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(4, &survivors).unwrap(),
        certificate: CollapseCertificate {
            algorithm_version: 2,
            objective: None,
            completeness: CollapseCompleteness::CompleteFixedPoint,
            work_limit: None,
            work_used: 0,
            vertex_count: 4,
            requested_threshold: None,
            terminal_level: 1.0,
            input_edge_count: 6,
            output_edge_count: survivors.len(),
            steps,
        },
        stats: stats_for(6, survivors.len()),
        timings: Default::default(),
    }
}
