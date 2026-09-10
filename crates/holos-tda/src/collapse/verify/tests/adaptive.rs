use super::common::{k4_dense, triangle_dense, triangle_result, two_k4_dense, two_k4_v2_result};

use crate::SparseDistanceMatrix;
use crate::collapse::verify::{verify_dense, verify_sparse};
use crate::collapse::{
    AdaptiveCollapseParams, CollapseCompleteness, CollapseObjective, SchedulePosition,
    collapse_dense_adaptive, collapse_sparse_rounds_parallel,
};

#[test]
fn rejects_unknown_algorithm_version() {
    for version in [0, 4] {
        let mut result = two_k4_v2_result();
        result.certificate.algorithm_version = version;
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, None);
        assert!(
            err.message
                .contains(&format!("unsupported algorithm version {version}")),
            "{}",
            err.message
        );
    }
}

#[test]
fn accepts_budget_limited_adaptive_certificate_without_a_fixed_point() {
    let result = collapse_dense_adaptive(
        &triangle_dense(),
        None,
        AdaptiveCollapseParams::new(CollapseObjective::H1).with_work_limit(0),
    )
    .unwrap();
    assert_eq!(
        result.certificate.completeness(),
        CollapseCompleteness::BudgetLimited
    );
    assert_eq!(verify_dense(&triangle_dense(), None, &result), Ok(()));
}

#[test]
fn rejects_inconsistent_adaptive_metadata() {
    let complete = collapse_dense_adaptive(
        &triangle_dense(),
        None,
        AdaptiveCollapseParams::new(CollapseObjective::H1),
    )
    .unwrap();

    let mut no_objective = complete.clone();
    no_objective.certificate.objective = None;
    let err = verify_dense(&triangle_dense(), None, &no_objective).unwrap_err();
    assert!(
        err.message.contains("no collapse objective"),
        "{}",
        err.message
    );

    let mut no_limit = complete.clone();
    no_limit.certificate.completeness = CollapseCompleteness::BudgetLimited;
    let err = verify_dense(&triangle_dense(), None, &no_limit).unwrap_err();
    assert!(err.message.contains("no work limit"), "{}", err.message);

    let mut under_limit = complete.clone();
    under_limit.certificate.completeness = CollapseCompleteness::BudgetLimited;
    under_limit.certificate.work_limit = Some(under_limit.certificate.work_used + 1);
    let err = verify_dense(&triangle_dense(), None, &under_limit).unwrap_err();
    assert!(
        err.message.contains("expected its limit"),
        "{}",
        err.message
    );

    let mut over_limit = complete;
    over_limit.certificate.work_limit = Some(0);
    let err = verify_dense(&triangle_dense(), None, &over_limit).unwrap_err();
    assert!(err.message.contains("exceeds its limit"), "{}", err.message);
}

#[test]
fn rejects_adaptive_sequence_gap() {
    let mut result = collapse_dense_adaptive(
        &k4_dense(),
        None,
        AdaptiveCollapseParams::new(CollapseObjective::H2),
    )
    .unwrap();
    result.certificate.steps[1].position = SchedulePosition::Sequence(3);
    let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(1));
    assert!(err.message.contains("sequence position"), "{}", err.message);
}

#[test]
fn rejects_position_kind_for_each_algorithm_version() {
    let mut v1 = triangle_result();
    v1.certificate.steps[0].position = SchedulePosition::Round(1);
    let err = verify_dense(&triangle_dense(), None, &v1).unwrap_err();
    assert!(
        err.message.contains("not positioned in a pass"),
        "{}",
        err.message
    );

    let mut v2 = two_k4_v2_result();
    v2.certificate.steps[0].position = SchedulePosition::Pass(1);
    let err = verify_dense(&two_k4_dense(), None, &v2).unwrap_err();
    assert!(
        err.message.contains("not positioned in a round"),
        "{}",
        err.message
    );

    let mut v3 = collapse_dense_adaptive(
        &triangle_dense(),
        None,
        AdaptiveCollapseParams::new(CollapseObjective::H1),
    )
    .unwrap();
    v3.certificate.steps[0].position = SchedulePosition::Pass(1);
    let err = verify_dense(&triangle_dense(), None, &v3).unwrap_err();
    assert!(
        err.message
            .contains("not positioned in an adaptive sequence"),
        "{}",
        err.message
    );
}

#[test]
fn wide_independent_round_verifies_in_linear_time() {
    // Many disjoint unit K4s: round 1 removes one edge per block, so
    // the round is as wide as the block count. The independence check
    // must not compare every pair of steps.
    let blocks = 3000;
    let n = 4 * blocks;
    let mut triplets = Vec::new();
    for b in 0..blocks {
        let base = 4 * b;
        for i in 0..4 {
            for j in (i + 1)..4 {
                triplets.push((base + i, base + j, 1.0));
            }
        }
    }
    let dist = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
    let result = collapse_sparse_rounds_parallel(&dist, None, 1).unwrap();
    assert!(result.certificate.steps().len() >= blocks);
    let start = std::time::Instant::now();
    verify_sparse(&dist, None, &result).unwrap();
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "verifier took {:?} on a round of width {blocks}",
        start.elapsed()
    );
}
