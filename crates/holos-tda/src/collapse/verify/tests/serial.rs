use super::common::{
    k4_dense, k4_result, stats_for, triangle_dense, triangle_result, triangle_result_threshold_two,
    triangle_sparse, two_level_dense, two_level_result,
};

use crate::collapse::verify::{verify_dense, verify_sparse};
use crate::collapse::{
    CollapseCertificate, CollapseCompleteness, CollapsedRips, RemovalStep, SchedulePosition,
};
use crate::{DistanceMatrix, SparseDistanceMatrix};

#[test]
fn accepts_triangle_removal_dense() {
    assert_eq!(
        verify_dense(&triangle_dense(), None, &triangle_result()),
        Ok(())
    );
}

#[test]
fn accepts_triangle_removal_sparse() {
    assert_eq!(
        verify_sparse(&triangle_sparse(), None, &triangle_result()),
        Ok(())
    );
}

#[test]
fn rejects_wrong_edge_value() {
    let mut result = triangle_result();
    result.certificate.steps[0].value = 2.0;
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("value mismatch"), "{}", err.message);
}

#[test]
fn rejects_endpoint_witness_apex() {
    let mut result = triangle_result();
    result.certificate.steps[0].witnesses = vec![(1.0, 1)];
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("apex"), "{}", err.message);
}

#[test]
fn rejects_non_dominating_apex() {
    // Two apex candidates 2 and 3 that are not neighbors of each other:
    // neither dominates, so no removal of (0, 1) can verify.
    let dist =
        DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0, 1.0, 1.0, f64::INFINITY]).unwrap();
    let result = CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 2, 1.0), (1, 2, 1.0), (0, 3, 1.0), (1, 3, 1.0)],
        )
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
            input_edge_count: 5,
            output_edge_count: 4,
            steps: vec![RemovalStep {
                u: 0,
                v: 1,
                value: 1.0,
                position: SchedulePosition::Pass(1),
                witnesses: vec![(1.0, 2)],
            }],
        },
        stats: stats_for(5, 4),
        timings: Default::default(),
    };
    let err = verify_dense(&dist, None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("dominate"), "{}", err.message);
}

#[test]
fn rejects_segment_start_after_edge_value() {
    let mut result = triangle_result();
    result.certificate.steps[0].witnesses = vec![(1.5, 2)];
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("starts at"), "{}", err.message);
}

#[test]
fn rejects_missing_step() {
    let mut result = triangle_result();
    result.certificate.steps.clear();
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, None);
    assert!(err.message.contains("invariant"), "{}", err.message);
}

#[test]
fn rejects_altered_output_edge() {
    let mut result = triangle_result();
    result.matrix = SparseDistanceMatrix::from_triplets(3, &[(0, 2, 1.5), (1, 2, 1.0)]).unwrap();
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, None);
    assert!(err.message.contains("value mismatch"), "{}", err.message);
}

#[test]
fn rejects_extra_output_edge() {
    let mut result = triangle_result();
    result.matrix = triangle_sparse();
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, None);
    assert!(err.message.contains("output edge count"), "{}", err.message);
}

#[test]
fn rejects_missed_removable_edge() {
    // Empty certificate on the full triangle: every edge is removable,
    // so the fixed-point scan must object.
    let result = CollapsedRips {
        matrix: triangle_sparse(),
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
            output_edge_count: 3,
            steps: vec![],
        },
        stats: stats_for(3, 3),
        timings: Default::default(),
    };
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, None);
    assert!(err.message.contains("removable"), "{}", err.message);
}

#[test]
fn accepts_isolated_edge_with_empty_certificate() {
    // A single edge has no common neighbor, so it is not removable and
    // the empty certificate is the correct fixed point.
    let dist = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
    let result = CollapsedRips {
        matrix: SparseDistanceMatrix::from_triplets(2, &[(0, 1, 1.0)]).unwrap(),
        certificate: CollapseCertificate {
            algorithm_version: 1,
            objective: None,
            completeness: CollapseCompleteness::CompleteFixedPoint,
            work_limit: None,
            work_used: 0,
            vertex_count: 2,
            requested_threshold: None,
            terminal_level: 1.0,
            input_edge_count: 1,
            output_edge_count: 1,
            steps: vec![],
        },
        stats: stats_for(1, 1),
        timings: Default::default(),
    };
    assert_eq!(verify_dense(&dist, None, &result), Ok(()));
}

#[test]
fn rejects_zero_pass_number() {
    let mut result = triangle_result();
    result.certificate.steps[0].position = SchedulePosition::Pass(0);
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("1-based"), "{}", err.message);
}

#[test]
fn rejects_equal_segment_starts() {
    let mut result = triangle_result();
    result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.0, 2)];
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(
        err.message.contains("strictly increasing"),
        "{}",
        err.message
    );
}

#[test]
fn rejects_wrong_terminal_level() {
    let mut result = triangle_result();
    result.certificate.terminal_level = 2.0;
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, None);
    assert!(err.message.contains("terminal level"), "{}", err.message);
}

#[test]
fn rejects_requested_threshold_mismatch() {
    let err = verify_dense(&triangle_dense(), Some(1.0), &triangle_result()).unwrap_err();
    assert_eq!(err.step, None);
    assert!(
        err.message.contains("requested threshold"),
        "{}",
        err.message
    );
}

#[test]
fn accepts_triangle_removal_below_threshold() {
    assert_eq!(
        verify_dense(
            &triangle_dense(),
            Some(2.0),
            &triangle_result_threshold_two()
        ),
        Ok(())
    );
}

#[test]
fn rejects_forged_appended_segment() {
    // The reviewed forgery: at threshold 2 the only critical value is
    // 1.0, so the appended segment (1.5, 0) was never the active
    // segment and slipped through unchecked.
    let mut result = triangle_result_threshold_two();
    result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.5, 0)];
    let err = verify_dense(&triangle_dense(), Some(2.0), &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("critical value"), "{}", err.message);
}

#[test]
fn rejects_segment_start_after_terminal() {
    let mut result = triangle_result();
    result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.5, 2)];
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("terminal level"), "{}", err.message);
}

#[test]
fn rejects_segment_at_non_critical_value() {
    // Two segments fit under the two critical values 1.0 and 2.0, but
    // 1.5 is not one of them.
    let mut result = two_level_result();
    result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.5, 3)];
    let err = verify_dense(&two_level_dense(), Some(2.0), &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(
        err.message.contains("not a critical value"),
        "{}",
        err.message
    );
}

#[test]
fn rejects_redundant_segment() {
    // Apex 2 still dominates at 2.0, so the frozen rule keeps it and
    // never opens the recorded segment (2.0, 3).
    let mut result = two_level_result();
    result.certificate.steps[0].witnesses = vec![(1.0, 2), (2.0, 3)];
    let err = verify_dense(&two_level_dense(), Some(2.0), &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("redundant"), "{}", err.message);
}

#[test]
fn accepts_k4_collapse() {
    assert_eq!(verify_dense(&k4_dense(), None, &k4_result()), Ok(()));
}

#[test]
fn rejects_apex_not_first_in_vertex_order() {
    // Vertices 2 and 3 both dominate (0, 1) at 1.0; the frozen rule
    // takes 2, so recording 3 is a different selection.
    let mut result = k4_result();
    result.certificate.steps[0].witnesses = vec![(1.0, 3)];
    let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("first dominating"), "{}", err.message);
}

#[test]
fn rejects_out_of_schedule_order_within_pass() {
    // Swapping (0, 2) and (1, 2) still replays cleanly, but pass 1
    // must visit combinadic index 1 before index 2.
    let mut result = k4_result();
    result.certificate.steps.swap(1, 2);
    let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(2));
    assert!(err.message.contains("schedule order"), "{}", err.message);
}

#[test]
fn rejects_first_pass_not_one() {
    let mut result = triangle_result();
    result.certificate.steps[0].position = SchedulePosition::Pass(2);
    let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("first step"), "{}", err.message);
}

#[test]
fn accepts_production_certificates() {
    // The verifier certifies the frozen rules exactly, so nothing the
    // production collapser emits may fail. Sweep small graphs with
    // ties, zeros, and +inf entries under several thresholds.
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let values = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
    for n in 3..8usize {
        for round in 0..8u64 {
            let m = n * (n - 1) / 2;
            let cond: Vec<f64> = (0..m)
                .map(|_| values[(next() % values.len() as u64) as usize])
                .collect();
            let dense = DistanceMatrix::from_condensed(cond).unwrap();
            let threshold = match round % 3 {
                0 => None,
                1 => Some(2.0),
                _ => Some(f64::INFINITY),
            };
            let r = crate::collapse::collapse_dense(&dense, threshold).unwrap();
            verify_dense(&dense, threshold, &r).unwrap();

            let triplets: Vec<(usize, usize, f64)> = (0..n)
                .flat_map(|u| (u + 1..n).map(move |v| (u, v)))
                .map(|(u, v)| (u, v, dense.get(u, v)))
                .filter(|&(_, _, d)| d.is_finite())
                .collect();
            let sparse = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            let r = crate::collapse::collapse_sparse(&sparse, threshold).unwrap();
            verify_sparse(&sparse, threshold, &r).unwrap();
        }
    }
}

#[test]
fn rejects_pass_gap() {
    let mut result = k4_result();
    result.certificate.steps[1].position = SchedulePosition::Pass(3);
    result.certificate.steps[2].position = SchedulePosition::Pass(3);
    let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(1));
    assert!(err.message.contains("skips pass 2"), "{}", err.message);
}
