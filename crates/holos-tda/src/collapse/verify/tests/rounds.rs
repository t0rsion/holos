use super::common::{k4_dense, k4_v2_round, two_k4_dense, two_k4_v2_result, v2_step};

use crate::collapse::SchedulePosition;
use crate::collapse::verify::verify_dense;

#[test]
fn accepts_two_k4_v2_rounds() {
    assert_eq!(
        verify_dense(&two_k4_dense(), None, &two_k4_v2_result()),
        Ok(())
    );
}

#[test]
fn rejects_same_round_conflict_despite_serial_validity() {
    // The serial-safe forgery: (0, 1) apex 2 then (0, 2) apex 3
    // replay cleanly one after the other, but both endpoints of
    // (0, 2) lie in S((0, 1)) = {0, 1, 2, 3}, so one round cannot
    // hold both.
    let result = k4_v2_round(vec![v2_step(0, 1, 1, 2), v2_step(0, 2, 1, 3)]);
    let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(1));
    assert!(err.message.contains("conflict"), "{}", err.message);
}

#[test]
fn rejects_witnesses_valid_only_after_earlier_step() {
    // Step 1 records apex 3 for (1, 2). The frozen rule selects 3
    // only after (0, 1) is gone; against the round snapshot the apex
    // is vertex 0, so a verifier that mutates between steps would
    // accept this pair. Any such in-round dependence puts both
    // endpoints of (1, 2) inside S((0, 1)), so the nonconflict check
    // rejects the grouping.
    let result = k4_v2_round(vec![v2_step(0, 1, 1, 2), v2_step(1, 2, 1, 3)]);
    let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(1));
    assert!(err.message.contains("conflict"), "{}", err.message);
}

#[test]
fn rejects_witnesses_from_stale_snapshot() {
    // Round 2 records apex 1 for (0, 2). Vertex 1 was the frozen
    // choice in the round 1 graph, but round 1 deleted (0, 1), so in
    // the round 2 snapshot vertex 1 is no longer a common neighbor.
    let mut result = two_k4_v2_result();
    result.certificate.steps[2].witnesses = vec![(1.0, 1)];
    let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(2));
    assert!(err.message.contains("common neighbor"), "{}", err.message);
}

#[test]
fn rejects_round_gap() {
    let mut result = two_k4_v2_result();
    for step in &mut result.certificate.steps[2..] {
        step.position = SchedulePosition::Round(3);
    }
    let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(2));
    assert!(err.message.contains("skips round 2"), "{}", err.message);
}

#[test]
fn rejects_interleaved_rounds() {
    // Round tags 1, 2, 1: the third step returns to a closed round.
    let mut result = two_k4_v2_result();
    result.certificate.steps.swap(1, 2);
    let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(2));
    assert!(err.message.contains("decreases"), "{}", err.message);
}

#[test]
fn rejects_out_of_order_within_round() {
    // (4, 5) before (0, 1) replays cleanly but breaks the frozen
    // in-round order: equal values must go by ascending (v, u).
    let mut result = two_k4_v2_result();
    result.certificate.steps.swap(0, 1);
    let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(1));
    assert!(err.message.contains("schedule order"), "{}", err.message);
}

#[test]
fn rejects_first_round_not_one() {
    let mut result = two_k4_v2_result();
    result.certificate.steps[0].position = SchedulePosition::Round(2);
    let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("first step"), "{}", err.message);
}

#[test]
fn rejects_round_zero() {
    let mut result = two_k4_v2_result();
    result.certificate.steps[0].position = SchedulePosition::Round(0);
    let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
    assert_eq!(err.step, Some(0));
    assert!(err.message.contains("round number 0"), "{}", err.message);
}
