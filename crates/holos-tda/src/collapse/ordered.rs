//! The ordered schedule: speculative parallel execution of the serial
//! schedule, written as algorithm version 1 certificates.
//!
//! Workers test windows of due edges in parallel against a frozen graph
//! state. The retirement walk then commits them strictly in the serial
//! order: it reuses a cached verdict only when no removal committed since
//! the test conflicts with it, and recomputes an invalidated verdict once
//! against the current graph. Every worker count therefore produces the
//! serial matrix and certificate, field for field, floats bit for bit.
//! Only the work counters and the timings depend on the window and
//! worker configuration.

use std::time::{Duration, Instant};

use rayon::prelude::*;

use super::{
    CollapseStats, CollapseTimings, CollapsedRips, Execution, Prepared, RemovalStep,
    SchedulePosition, Scratch, build_pool, finish, mark_dirty, prepare, test_edge, tombstone,
};
use crate::distances::Distances;
use crate::{DistanceMatrix, Result, SparseDistanceMatrix};

/// Window members per worker. Chosen from disclosed development
/// measurements (three dev clouds, 4 workers, k in {2, 8, 32, 128, 512}:
/// 32 was the best or tied-best everywhere) and frozen before the
/// registered screen. It changes work, memory, and timing only, never an
/// output field.
const WINDOW_PER_WORKER: usize = 32;

/// Upper bound on the window, so speculative storage stays small on high
/// worker counts.
const WINDOW_CAP: usize = 4096;

/// The piecewise witness segments of one removable edge.
type Witnesses = Vec<(f64, usize)>;

/// The production window for a worker count.
fn window_for(workers: usize) -> usize {
    (WINDOW_PER_WORKER * workers.max(1)).clamp(1, WINDOW_CAP)
}

/// A duration in nanoseconds, saturated at the reported field's width.
fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Run the ordered schedule on a dense distance matrix.
///
/// Workers test a window of due edges in parallel, then the run retires
/// them in serial order. `threshold` follows the engine's rule: `None` means the
/// enclosing radius. The output equals the output of
/// [`super::collapse_dense`] at any `threads`; 0 and 1 run the serial
/// implementation. A standalone call owns its thread pool for the
/// duration.
pub fn collapse_dense_ordered_parallel(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, None)
}

/// Run the ordered schedule on a sparse distance matrix.
///
/// Same contract as [`collapse_dense_ordered_parallel`]; `None` keeps
/// every listed edge.
pub fn collapse_sparse_ordered_parallel(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, None)
}

/// Run the ordered schedule on a dense distance matrix with a forced
/// window size.
///
/// Hidden and unstable. It exists for the invariance gates, which cross
/// worker counts with window sizes; the window is not public API and no
/// output field depends on it. A `window` of 0 becomes 1.
#[doc(hidden)]
pub fn collapse_dense_ordered_with_window(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, Some(window.max(1)))
}

/// Run the ordered schedule on a sparse distance matrix with a forced
/// window size.
///
/// Same contract as [`collapse_dense_ordered_with_window`]: hidden,
/// unstable, and for the invariance gates only.
#[doc(hidden)]
pub fn collapse_sparse_ordered_with_window(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, Some(window.max(1)))
}

/// Build an owned pool for the call and run the ordered collapse on it.
/// One worker has nothing to speculate on, so it runs the shipped serial
/// implementation.
fn collapse_ordered_owned<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    threads: usize,
    window: Option<usize>,
) -> Result<CollapsedRips> {
    if threads.max(1) == 1 {
        return serial(dist, threshold);
    }
    let pool = build_pool(threads)?;
    let w = window.unwrap_or_else(|| window_for(pool.current_num_threads()));
    collapse_ordered_core(dist, threshold, Some(&pool), w)
}

/// The ordered collapse on a caller-provided pool (`None` runs the
/// serial implementation). The pipeline shares its run-wide pool
/// through here; the public wrappers build and own one.
pub(crate) fn collapse_ordered_in<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    pool: Option<&rayon::ThreadPool>,
) -> Result<CollapsedRips> {
    match pool.filter(|p| p.current_num_threads() > 1) {
        Some(p) => collapse_ordered_core(
            dist,
            threshold,
            Some(p),
            window_for(p.current_num_threads()),
        ),
        None => serial(dist, threshold),
    }
}

/// The serial version 1 run, reported as an ordered run. Its logical
/// trace is its own test sequence, so the logical count is the physical
/// count.
///
/// The run has no stages, so it keeps the window counters at zero and the
/// default timings the serial implementation reports.
fn serial<D: Distances>(dist: &D, threshold: Option<f64>) -> Result<CollapsedRips> {
    super::collapse_impl(dist, threshold)
}

/// The staged-window execution of the serial schedule.
///
/// A pass runs as a sequence of stages. FORM collects up to `window` due
/// positions from the cursor without touching a dirty flag. TEST
/// evaluates the predicate for every member against the frozen graph.
/// RETIRE walks every position of the window span in schedule order and
/// commits it exactly as version 1 would, reusing a member's cached
/// verdict when no earlier removal of this stage conflicts with it.
fn collapse_ordered_core<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    pool: Option<&rayon::ThreadPool>,
    window: usize,
) -> Result<CollapsedRips> {
    let Prepared {
        mut edges,
        mut adj,
        run,
    } = prepare(dist, threshold)?;
    let window = window.max(1);

    let mut stats = CollapseStats::new(edges.len());
    let mut steps: Vec<RemovalStep> = Vec::new();
    let mut scratch = Scratch::default();
    // Same pruning contract as v1: a pass tests the edges the previous
    // removals marked dirty, or every live edge after a removal whose
    // neighborhood was too large to mark finely.
    let mut dirty: Vec<bool> = vec![false; edges.len()];
    let mut test_all = true;
    let mut members: Vec<usize> = Vec::new();
    let mut stale: Vec<bool> = Vec::new();
    let mut predicate_time = Duration::ZERO;
    let mut retirement_time = Duration::ZERO;
    let mut repair_time = Duration::ZERO;
    loop {
        stats.epochs += 1;
        let mut removed_any = false;
        let mut test_all_next = false;
        let mut c = 0usize;
        while c < edges.len() {
            // FORM. Dirty flags stay as they are: a member is due because
            // its flag is set, and only its own retirement may clear it.
            members.clear();
            let mut scan = c;
            while scan < edges.len() && members.len() < window {
                if edges[scan].alive && (test_all || dirty[scan]) {
                    members.push(scan);
                }
                scan += 1;
            }
            // No due position remains ahead: the pass ends here.
            let Some(&last) = members.last() else {
                break;
            };
            stats.window_batches += 1;
            stats.window_slots_offered = stats.window_slots_offered.saturating_add(window);
            stats.window_members_formed += members.len();

            // TEST. Read-only against the frozen graph, collected in
            // member order, so the worker count cannot reach the output.
            stats.edge_tests += members.len();
            let predicate_start = Instant::now();
            let mut cached: Vec<(Option<Witnesses>, usize)> = match pool {
                Some(pool) => pool.install(|| {
                    members
                        .par_iter()
                        .map_init(Scratch::default, |s, &idx| {
                            let e = &edges[idx];
                            let w = test_edge(&adj, e.u, e.v, e.value, run.terminal, s);
                            (w, s.cands.len())
                        })
                        .collect()
                }),
                None => members
                    .iter()
                    .map(|&idx| {
                        let e = &edges[idx];
                        let w = test_edge(&adj, e.u, e.v, e.value, run.terminal, &mut scratch);
                        (w, scratch.cands.len())
                    })
                    .collect(),
            };
            predicate_time += predicate_start.elapsed();
            for &(_, k) in &cached {
                stats.max_common_neighborhood = stats.max_common_neighborhood.max(k);
            }
            stale.clear();
            stale.resize(members.len(), false);

            // RETIRE. Every position of the span, in schedule order, with
            // v1's state at each step: the graph, the dirty flags,
            // `test_all`, and the pass number.
            let retirement_start = Instant::now();
            let mut next = 0usize;
            for j in c..=last {
                let slot = if next < members.len() && members[next] == j {
                    next += 1;
                    Some(next - 1)
                } else {
                    None
                };
                // A removal clears only its own `alive` flag, and RETIRE
                // clears a dirty flag only at the position it retires, so
                // a member is still alive and still due at its turn. A
                // non-member here was armed after FORM.
                if !edges[j].alive || !(test_all || dirty[j]) {
                    continue;
                }
                stats.logical_tests += 1;
                dirty[j] = false;
                let (u, v, value) = (edges[j].u, edges[j].v, edges[j].value);
                let witnesses = match slot.filter(|&k| !stale[k]) {
                    Some(k) => {
                        stats.window_members_reused += 1;
                        cached[k].0.take()
                    }
                    None => {
                        stats.edge_tests += 1;
                        // A non-member here was armed after FORM, so its
                        // serial test repairs nothing. Every stale member
                        // comes through here, so `invalidated_results` is
                        // also the repair count.
                        let repairing = slot.is_some();
                        let repair_start = repairing.then(Instant::now);
                        let w = test_edge(&adj, u, v, value, run.terminal, &mut scratch);
                        if let Some(start) = repair_start {
                            repair_time += start.elapsed();
                        }
                        stats.max_common_neighborhood =
                            stats.max_common_neighborhood.max(scratch.cands.len());
                        w
                    }
                };
                let Some(witnesses) = witnesses else {
                    continue;
                };
                edges[j].alive = false;
                if mark_dirty(&adj, &mut dirty, u, v, &mut scratch) {
                    // Staleness cannot be read back from `dirty`: a member
                    // still ahead has had its flag set since FORM, so a
                    // set flag says nothing about this removal. A true
                    // return leaves `marks` holding exactly the sorted S
                    // that the marking walked, against this same
                    // pre-tombstone graph, so the stale set is read from
                    // there. Both false returns leave it unusable and take
                    // the branch below.
                    let set = &scratch.marks;
                    for (k, &pos) in members.iter().enumerate().skip(next) {
                        if stale[k] {
                            continue;
                        }
                        let m = &edges[pos];
                        if set.binary_search(&m.u).is_ok() && set.binary_search(&m.v).is_ok() {
                            stale[k] = true;
                            stats.invalidated_results += 1;
                        }
                    }
                } else {
                    // The marking bailed and left no usable set behind, so
                    // the conflict set is unknown: every cached result
                    // still ahead goes.
                    test_all_next = true;
                    let mut dropped = 0usize;
                    for st in stale.iter_mut().skip(next) {
                        if !*st {
                            *st = true;
                            dropped += 1;
                        }
                    }
                    if dropped > 0 {
                        stats.global_invalidations += 1;
                        stats.invalidated_results += dropped;
                    }
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
            retirement_time += retirement_start.elapsed();
            // The cursor stops after the last member, not where the FORM
            // scan stopped: a position the scan passed over may have been
            // armed during RETIRE, and the next stage must see it.
            c = last + 1;
        }
        if !removed_any {
            break;
        }
        test_all = test_all_next;
    }

    let timings = CollapseTimings {
        predicate_ns: nanos(predicate_time),
        retirement_ns: nanos(retirement_time),
        repair_ns: nanos(repair_time),
    };
    finish(run, Execution::Ordered, &edges, steps, stats, timings)
}

#[cfg(test)]
mod tests {
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
        SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
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
                        collapse_dense_ordered_with_window(&random_two_value_40(), None, t, w)
                            .unwrap(),
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
}
