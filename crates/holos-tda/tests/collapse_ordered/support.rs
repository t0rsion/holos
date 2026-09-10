use super::*;

/// Worker counts the invariance gate crosses. 0 and 1 delegate to the
/// serial implementation, so they pin the delegation path too.
pub(crate) const WORKERS: [usize; 5] = [0, 1, 2, 4, 8];
/// Forced window sizes: below every worker count, at a plausible
/// production size, and larger than any input here. The production window
/// enters through the entry point that takes no window.
pub(crate) const WINDOWS: [usize; 4] = [1, 2, 64, 100_000];
/// A window no fixture can fill, so a pass runs as one stage.
pub(crate) const ONE_STAGE: usize = 100_000;
pub(crate) const MODULI: [u32; 3] = [2, 3, 5];
pub(crate) const ALL_ON: (bool, bool, bool) = (true, true, true);
pub(crate) const ALL_OFF: (bool, bool, bool) = (false, false, false);

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub(crate) fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Dense matrix from an explicit edge list. Every unlisted pair is +inf.
pub(crate) fn dense_from_edges(n: usize, edges: &[(usize, usize, f64)]) -> DistanceMatrix {
    let mut full = vec![f64::INFINITY; n * n];
    for &(u, v, d) in edges {
        full[u * n + v] = d;
        full[v * n + u] = d;
    }
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            condensed.push(full[i * n + j]);
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

/// The sparse form of a dense matrix: every finite entry becomes an edge.
pub(crate) fn sparse_from_dense(dist: &DistanceMatrix) -> SparseDistanceMatrix {
    let n = dist.len();
    let mut triplets = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = dist.get(i, j);
            if d.is_finite() {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
}

pub(crate) fn edge_bits(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, u64)> {
    matrix
        .edges()
        .map(|(u, v, d)| (u, v, d.to_bits()))
        .collect()
}

/// Every certificate step as plain data, floats by bits.
pub(crate) type StepBits = ((usize, usize), u64, usize, Vec<(u64, usize)>);

pub(crate) fn step_bits(result: &CollapsedRips) -> Vec<StepBits> {
    result
        .certificate
        .steps()
        .iter()
        .map(|s| {
            (
                s.edge(),
                s.value().to_bits(),
                s.position().number(),
                s.witnesses()
                    .iter()
                    .map(|&(t, w)| (t.to_bits(), w))
                    .collect(),
            )
        })
        .collect()
}

pub(crate) fn step_for(
    result: &CollapsedRips,
    edge: (usize, usize),
) -> Option<&holos_tda::collapse::RemovalStep> {
    result.certificate.steps().iter().find(|s| s.edge() == edge)
}

/// Full output equality: the matrix and every certificate field, floats by
/// bits.
pub(crate) fn assert_same_output(name: &str, got: &CollapsedRips, want: &CollapsedRips) {
    let a = &got.certificate;
    let b = &want.certificate;
    assert_eq!(
        a.algorithm_version(),
        b.algorithm_version(),
        "{name}: algorithm version"
    );
    assert_eq!(a.vertex_count(), b.vertex_count(), "{name}: vertex count");
    assert_eq!(
        a.requested_threshold().map(f64::to_bits),
        b.requested_threshold().map(f64::to_bits),
        "{name}: requested threshold"
    );
    assert_eq!(
        a.terminal_level().to_bits(),
        b.terminal_level().to_bits(),
        "{name}: terminal level"
    );
    assert_eq!(
        a.input_edge_count(),
        b.input_edge_count(),
        "{name}: input edge count"
    );
    assert_eq!(
        a.output_edge_count(),
        b.output_edge_count(),
        "{name}: output edge count"
    );
    assert_eq!(step_bits(got), step_bits(want), "{name}: certificate steps");
    assert_eq!(
        edge_bits(&got.matrix),
        edge_bits(&want.matrix),
        "{name}: output matrix"
    );
    assert_eq!(got.stats.epochs, want.stats.epochs, "{name}: passes");
}

/// The counters that may not move with the worker count or the window.
pub(crate) fn assert_invariant_stats(name: &str, got: &CollapsedRips, want: &CollapsedRips) {
    assert_eq!(
        got.stats.input_edges, want.stats.input_edges,
        "{name}: input_edges"
    );
    assert_eq!(
        got.stats.output_edges, want.stats.output_edges,
        "{name}: output_edges"
    );
    assert_eq!(
        got.stats.removed_edges, want.stats.removed_edges,
        "{name}: removed_edges"
    );
    assert_eq!(got.stats.epochs, want.stats.epochs, "{name}: passes");
    assert_eq!(
        got.stats.witness_segments, want.stats.witness_segments,
        "{name}: witness_segments"
    );
    assert_eq!(
        got.stats.logical_tests, want.stats.logical_tests,
        "{name}: logical_tests"
    );
}

/// The work bound of the specification: one logical test costs at most one
/// speculative evaluation plus one serial repair.
pub(crate) fn assert_work_bound(name: &str, result: &CollapsedRips) {
    assert!(
        result.stats.edge_tests >= result.stats.logical_tests,
        "{name}: {} physical tests below {} logical tests",
        result.stats.edge_tests,
        result.stats.logical_tests
    );
    assert!(
        result.stats.edge_tests <= 2 * result.stats.logical_tests,
        "{name}: {} physical tests exceed twice {} logical tests",
        result.stats.edge_tests,
        result.stats.logical_tests
    );
}

/// Every window member is retired exactly once, from its cached verdict
/// or through a repair. A member is alive and due when the window forms,
/// and only its own retirement can revoke either, so the identity is the
/// scheduler's state invariant, not an upper bound.
pub(crate) fn assert_occupancy(name: &str, r: &CollapsedRips) {
    let s = &r.stats;
    assert!(
        s.window_members_formed <= s.window_slots_offered,
        "{name}: formed {} exceeds offered {}",
        s.window_members_formed,
        s.window_slots_offered
    );
    assert_eq!(
        s.window_members_reused + s.invalidated_results,
        s.window_members_formed,
        "{name}: reused {} plus repairs {} is not formed {}",
        s.window_members_reused,
        s.invalidated_results,
        s.window_members_formed
    );
}

/// Ordered against the shipped serial version 1 run: same output, and the
/// logical test count is the serial test count.
pub(crate) fn assert_matches_serial(name: &str, ordered: &CollapsedRips, serial: &CollapsedRips) {
    assert_same_output(name, ordered, serial);
    assert_occupancy(name, ordered);
    assert_eq!(
        ordered.stats.logical_tests, serial.stats.edge_tests,
        "{name}: logical tests must equal the serial test count"
    );
    assert_eq!(
        ordered.certificate.algorithm_version(),
        1,
        "{name}: ordered output must carry a version 1 certificate"
    );
    assert_work_bound(name, ordered);
}
