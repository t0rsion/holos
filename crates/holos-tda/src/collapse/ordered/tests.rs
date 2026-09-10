use super::*;
use crate::collapse::{CollapseCertificate, collapse_dense, collapse_sparse};

const WINDOWS: [usize; 5] = [1, 2, 8, 64, 10_000];

fn edge_bits(m: &SparseDistanceMatrix) -> Vec<(usize, usize, u64)> {
    m.edges().map(|(u, v, d)| (u, v, d.to_bits())).collect()
}

/// Certificate equality with every float compared by bits, which
/// `PartialEq` does not give for +0.0 against -0.0 or for NaN.
fn assert_certificate_bits(a: &CollapseCertificate, b: &CollapseCertificate, label: &str) {
    assert_eq!(a.algorithm_version(), b.algorithm_version(), "{label}");
    assert_eq!(a.vertex_count(), b.vertex_count(), "{label}");
    assert_eq!(
        a.requested_threshold().map(f64::to_bits),
        b.requested_threshold().map(f64::to_bits),
        "{label}"
    );
    assert_eq!(
        a.terminal_level().to_bits(),
        b.terminal_level().to_bits(),
        "{label}"
    );
    assert_eq!(a.input_edge_count(), b.input_edge_count(), "{label}");
    assert_eq!(a.output_edge_count(), b.output_edge_count(), "{label}");
    assert_eq!(a.steps().len(), b.steps().len(), "{label}");
    for (x, y) in a.steps().iter().zip(b.steps()) {
        assert_eq!(x.edge(), y.edge(), "{label}");
        assert_eq!(x.value().to_bits(), y.value().to_bits(), "{label}");
        assert_eq!(x.position().number(), y.position().number(), "{label}");
        let wx: Vec<_> = x
            .witnesses()
            .iter()
            .map(|&(t, w)| (t.to_bits(), w))
            .collect();
        let wy: Vec<_> = y
            .witnesses()
            .iter()
            .map(|&(t, w)| (t.to_bits(), w))
            .collect();
        assert_eq!(wx, wy, "{label}");
    }
}

/// Occupancy relations that hold at every window and worker count.
/// The values themselves are configuration-dependent, so only the
/// relations are gated here.
fn assert_occupancy_bounds(r: &CollapsedRips, label: &str) {
    let s = &r.stats;
    assert!(
        s.window_members_formed <= s.window_slots_offered,
        "{label}: formed {} > offered {}",
        s.window_members_formed,
        s.window_slots_offered
    );
    assert!(
        s.window_members_reused <= s.window_members_formed,
        "{label}: reused {} > formed {}",
        s.window_members_reused,
        s.window_members_formed
    );
    // Every member is retired exactly once, either from its cached
    // verdict or through a repair. This is an equality, not a bound:
    // a member is alive and due at FORM, only its own retirement can
    // remove it, and only its own retirement clears its dirty flag,
    // so neither can be revoked before its turn. The proof's state
    // invariant rests on that, so gating the identity gates the
    // invariant.
    assert_eq!(
        s.window_members_reused + s.invalidated_results,
        s.window_members_formed,
        "{label}: reused {} plus repairs {} != formed {}",
        s.window_members_reused,
        s.invalidated_results,
        s.window_members_formed
    );
}

fn assert_matches_serial(ordered: &CollapsedRips, serial: &CollapsedRips, label: &str) {
    assert_occupancy_bounds(ordered, label);
    assert_eq!(ordered.certificate, serial.certificate, "{label}");
    assert_certificate_bits(&ordered.certificate, &serial.certificate, label);
    assert_eq!(
        edge_bits(&ordered.matrix),
        edge_bits(&serial.matrix),
        "{label}"
    );
    assert_eq!(ordered.stats.epochs, serial.stats.epochs, "{label}");
    assert_eq!(
        ordered.stats.logical_tests, serial.stats.edge_tests,
        "{label}"
    );
    assert_eq!(
        ordered.stats.input_edges, serial.stats.input_edges,
        "{label}"
    );
    assert_eq!(
        ordered.stats.output_edges, serial.stats.output_edges,
        "{label}"
    );
    assert_eq!(
        ordered.stats.removed_edges, serial.stats.removed_edges,
        "{label}"
    );
    assert_eq!(
        ordered.stats.witness_segments, serial.stats.witness_segments,
        "{label}"
    );
    assert!(
        ordered.stats.edge_tests >= ordered.stats.logical_tests,
        "{label}"
    );
}

// Complete graph on 20 vertices with values in 1..=5: heavy ties, so
// windows straddle many equal-priority edges.
fn tie_heavy_20() -> DistanceMatrix {
    let mut condensed = Vec::new();
    for i in 1..20usize {
        for j in 0..i {
            condensed.push(((i * j + i + j) % 5 + 1) as f64);
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

fn unit_k4() -> DistanceMatrix {
    DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap()
}

// 40 vertices, two edge values from a fixed linear congruential
// stream: long windows, deep passes, and many conflicts.
fn random_two_value_40() -> DistanceMatrix {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut condensed = Vec::new();
    for _ in 0..40 * 39 / 2 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        condensed.push(if (state >> 33) % 3 == 0 { 1.0 } else { 2.0 });
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

fn four_cycle() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)])
        .unwrap()
}

fn two_unit_k4s() -> SparseDistanceMatrix {
    let mut triplets = Vec::new();
    for base in [0usize, 4] {
        for v in 1..4 {
            for u in 0..v {
                triplets.push((base + u, base + v, 1.0));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(8, &triplets).unwrap()
}

#[test]
fn dense_ordered_matches_serial_across_threads_and_windows() {
    for (name, d) in [
        ("tie_heavy_20", tie_heavy_20()),
        ("k4", unit_k4()),
        ("random_40", random_two_value_40()),
    ] {
        let base = collapse_dense(&d, None).unwrap();
        assert!(base.stats.removed_edges > 0, "{name}");
        for w in WINDOWS {
            let r = collapse_ordered_core(&d, None, None, w).unwrap();
            assert_matches_serial(&r, &base, &format!("{name} pool=none w={w}"));
            assert!(r.stats.window_batches > 0);
            for t in [2usize, 4] {
                let r = collapse_dense_ordered_with_window(&d, None, t, w).unwrap();
                assert_matches_serial(&r, &base, &format!("{name} threads={t} w={w}"));
            }
        }
    }
}

#[test]
fn sparse_ordered_matches_serial_across_threads_and_windows() {
    for (name, m) in [("four_cycle", four_cycle()), ("two_k4s", two_unit_k4s())] {
        let base = collapse_sparse(&m, None).unwrap();
        for w in WINDOWS {
            let r = collapse_ordered_core(&m, None, None, w).unwrap();
            assert_matches_serial(&r, &base, &format!("{name} pool=none w={w}"));
            for t in [2usize, 4] {
                let r = collapse_sparse_ordered_with_window(&m, None, t, w).unwrap();
                assert_matches_serial(&r, &base, &format!("{name} threads={t} w={w}"));
            }
        }
    }
}

// Unit K4 with a window over the whole pass. All six edges test
// removable against the frozen graph, but after (0,1), (0,2), and
// (1,2) fall, the three edges at vertex 3 have no common neighbor
// left. Their cached verdicts are stale positives; the repairs turn
// them into refusals and the star at 3 survives, as in v1. Reusing a
// stale cache here would delete the whole graph.
#[test]
fn stale_member_repair_flips_a_verdict() {
    let d = unit_k4();
    let base = collapse_dense(&d, None).unwrap();
    let r = collapse_dense_ordered_with_window(&d, None, 2, 64).unwrap();
    assert_matches_serial(&r, &base, "k4 stale repair");
    let removed: Vec<_> = r.certificate.steps().iter().map(|s| s.edge()).collect();
    assert_eq!(removed, vec![(0, 1), (0, 2), (1, 2)]);
    assert!(r.stats.invalidated_results >= 1);
    assert!(r.stats.invalidated_results >= 1);
    assert!(r.stats.edge_tests > r.stats.logical_tests);
}

// Unit K4 in one stage of 64 slots. FORM offers the whole window and
// collects all six edges. Only (0,1) retires on its cached verdict:
// its removal touches every member behind it, so the other five are
// repaired. Pass 2 finds no due position and opens no stage.
#[test]
fn unit_k4_occupancy_is_exact() {
    let r = collapse_dense_ordered_with_window(&unit_k4(), None, 2, 64).unwrap();
    assert_eq!(r.stats.epochs, 2);
    assert_eq!(r.stats.window_batches, 1);
    assert_eq!(r.stats.window_slots_offered, 64);
    assert_eq!(r.stats.window_members_formed, 6);
    assert_eq!(r.stats.window_members_reused, 1);
    assert_eq!(r.stats.invalidated_results, 5);
    assert_eq!(r.stats.invalidated_results, 5);
    assert_eq!(r.stats.logical_tests, 6);
    assert_eq!(r.stats.edge_tests, 11);
}

#[test]
fn offered_slots_are_stages_times_the_window() {
    for w in WINDOWS {
        for t in [2usize, 4] {
            for (name, r) in [
                (
                    "tie_heavy_20",
                    collapse_dense_ordered_with_window(&tie_heavy_20(), None, t, w).unwrap(),
                ),
                (
                    "random_40",
                    collapse_dense_ordered_with_window(&random_two_value_40(), None, t, w).unwrap(),
                ),
                (
                    "two_k4s",
                    collapse_sparse_ordered_with_window(&two_unit_k4s(), None, t, w).unwrap(),
                ),
            ] {
                let label = format!("{name} threads={t} w={w}");
                assert!(r.stats.window_batches > 0, "{label}");
                assert_eq!(
                    r.stats.window_slots_offered,
                    r.stats.window_batches * w,
                    "{label}"
                );
                assert_occupancy_bounds(&r, &label);
                // A stage forms at least one member, or FORM would
                // have ended the pass instead of opening it.
                assert!(
                    r.stats.window_members_formed >= r.stats.window_batches,
                    "{label}"
                );
            }
        }
    }
}

// Phase clocks report real time, not zeros. The values move run to
// run, so only presence and containment are gated.
#[test]
fn ordered_timings_are_measured() {
    let d = random_two_value_40();
    let r = collapse_dense_ordered_with_window(&d, None, 2, 8).unwrap();
    // Containment only: a bare `> 0` would depend on the clock's
    // resolution.
    assert!(r.stats.window_batches > 1);
    assert!(r.timings.repair_ns <= r.timings.retirement_ns);

    let k4 = collapse_dense_ordered_with_window(&unit_k4(), None, 2, 64).unwrap();
    assert!(k4.stats.invalidated_results > 0);
    assert!(k4.timings.repair_ns <= k4.timings.retirement_ns);

    // One member per stage leaves nothing ahead of a removal, so no
    // verdict goes stale and no repair runs.
    let single = collapse_dense_ordered_with_window(&d, None, 2, 1).unwrap();
    assert_eq!(single.stats.invalidated_results, 0);
    assert_eq!(single.timings.repair_ns, 0);
}

#[test]
fn one_worker_delegates_to_the_serial_run() {
    let d = tie_heavy_20();
    let base = collapse_dense(&d, None).unwrap();
    for threads in [0usize, 1] {
        let r = collapse_dense_ordered_parallel(&d, None, threads).unwrap();
        assert_matches_serial(&r, &base, "delegation");
        assert_eq!(r.stats.edge_tests, base.stats.edge_tests);
        assert_eq!(r.stats.edge_tests, r.stats.logical_tests);
        assert_eq!(
            r.stats.max_common_neighborhood,
            base.stats.max_common_neighborhood
        );
        assert_eq!(r.stats.invalidated_results, 0);
        assert_eq!(r.stats.invalidated_results, 0);
        assert_eq!(r.stats.global_invalidations, 0);
        assert_eq!(r.stats.window_batches, 0);
        assert_eq!(r.stats.window_slots_offered, 0);
        assert_eq!(r.stats.window_members_formed, 0);
        assert_eq!(r.stats.window_members_reused, 0);
        assert_eq!(r.timings, CollapseTimings::default());
    }
    let m = two_unit_k4s();
    let sparse_base = collapse_sparse(&m, None).unwrap();
    let r = collapse_sparse_ordered_parallel(&m, None, 1).unwrap();
    assert_matches_serial(&r, &sparse_base, "sparse delegation");
}

// Complete graph on 66 vertices: S = N[u] intersect N[v] is the whole
// vertex set, above MARK_LIMIT, so the first removal bails out of the
// fine marking. The bail cannot name the conflicts, so the window
// remainder is invalidated wholesale and the pass still reproduces the
// serial trace.
#[test]
fn marking_bail_invalidates_the_window_remainder() {
    let d = DistanceMatrix::from_condensed(vec![1.0; 66 * 65 / 2]).unwrap();
    let base = collapse_dense(&d, None).unwrap();
    let r = collapse_dense_ordered_with_window(&d, None, 2, 64).unwrap();
    assert_matches_serial(&r, &base, "mark bail");
    assert!(r.stats.global_invalidations >= 1);
    assert!(r.stats.invalidated_results >= 1);
}

#[test]
fn empty_and_tiny_inputs() {
    let d0 = DistanceMatrix::from_points(&[]).unwrap();
    let d1 = DistanceMatrix::from_condensed(vec![]).unwrap();
    let s1 = SparseDistanceMatrix::from_triplets(1, &[]).unwrap();
    for threads in [0usize, 1, 4] {
        for r in [
            collapse_dense_ordered_parallel(&d0, None, threads).unwrap(),
            collapse_dense_ordered_parallel(&d1, None, threads).unwrap(),
            collapse_sparse_ordered_parallel(&s1, None, threads).unwrap(),
        ] {
            assert_eq!(r.certificate.algorithm_version(), 1);
            assert_eq!(r.certificate.input_edge_count(), 0);
            assert_eq!(r.certificate.terminal_level(), 0.0);
            assert!(r.certificate.steps().is_empty());
            assert_eq!(r.stats.epochs, 1);
            assert_eq!(r.stats.edge_tests, 0);
            assert_eq!(r.stats.logical_tests, 0);
            assert_eq!(r.stats.window_batches, 0);
            assert_eq!(r.stats.window_slots_offered, 0);
            assert_eq!(r.stats.window_members_formed, 0);
            assert_eq!(r.stats.window_members_reused, 0);
            assert_eq!(r.timings, CollapseTimings::default());
        }
    }
}

#[test]
fn invalid_thresholds_are_rejected() {
    let d = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
    let m = four_cycle();
    for threads in [1usize, 4] {
        assert!(collapse_dense_ordered_parallel(&d, Some(-1.0), threads).is_err());
        assert!(collapse_dense_ordered_parallel(&d, Some(f64::NAN), threads).is_err());
        assert!(collapse_sparse_ordered_parallel(&m, Some(-1.0), threads).is_err());
        assert!(collapse_sparse_ordered_parallel(&m, Some(f64::NAN), threads).is_err());
        assert!(collapse_dense_ordered_with_window(&d, Some(-1.0), threads, 4).is_err());
        assert!(collapse_sparse_ordered_with_window(&m, Some(f64::NAN), threads, 4).is_err());
    }
}
