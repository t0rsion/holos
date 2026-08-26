//! Ordered speculative collapse gates.
//!
//! The ordered execution runs the version 1 schedule with staged windows.
//! Its output must be the version 1 output: the same matrix, the same
//! certificate field for field with floats compared by bits, the same pass
//! count, at every worker count and every window size. Only the work
//! counters may move.
//!
//! Three references appear here: the shipped serial version 1 collapser,
//! an unpruned version 1 reference rebuilt from the specification, and the
//! uncollapsed engine with its brute-force oracle.
//!
//! The named fixtures at the end attack the scheduler itself. Each one is
//! built against the frozen edge order (value descending, ties by
//! `(v, u)` ascending), and its expectations come from hand-simulating the
//! serial schedule, not from a run.

use holos_tda::collapse::verify::{verify_dense, verify_sparse};
use holos_tda::collapse::{
    collapse_dense, collapse_dense_ordered_parallel, collapse_dense_ordered_with_window,
    collapse_sparse, collapse_sparse_ordered_parallel, collapse_sparse_ordered_with_window,
    CollapsedRips,
};
use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{
    rips_persistence, rips_persistence_sparse, Bar, CollapseSchedule, Diagram, DistanceMatrix,
    RipsParams, SparseDistanceMatrix,
};

/// Worker counts the invariance gate crosses. 0 and 1 delegate to the
/// serial implementation, so they pin the delegation path too.
const WORKERS: [usize; 5] = [0, 1, 2, 4, 8];
/// Forced window sizes: below every worker count, at a plausible
/// production size, and larger than any input here. The production window
/// enters through the entry point that takes no window.
const WINDOWS: [usize; 4] = [1, 2, 64, 100_000];
/// A window no fixture can fill, so a pass runs as one stage.
const ONE_STAGE: usize = 100_000;
const MODULI: [u32; 3] = [2, 3, 5];
const ALL_ON: (bool, bool, bool) = (true, true, true);
const ALL_OFF: (bool, bool, bool) = (false, false, false);

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Dense matrix from an explicit edge list. Every unlisted pair is +inf.
fn dense_from_edges(n: usize, edges: &[(usize, usize, f64)]) -> DistanceMatrix {
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
fn sparse_from_dense(dist: &DistanceMatrix) -> SparseDistanceMatrix {
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

fn edge_bits(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, u64)> {
    matrix
        .edges()
        .map(|(u, v, d)| (u, v, d.to_bits()))
        .collect()
}

/// Every certificate step as plain data, floats by bits.
type StepBits = ((usize, usize), u64, usize, Vec<(u64, usize)>);

fn step_bits(result: &CollapsedRips) -> Vec<StepBits> {
    result
        .certificate
        .steps()
        .iter()
        .map(|s| {
            (
                s.edge(),
                s.value().to_bits(),
                s.epoch(),
                s.witnesses()
                    .iter()
                    .map(|&(t, w)| (t.to_bits(), w))
                    .collect(),
            )
        })
        .collect()
}

fn step_for(
    result: &CollapsedRips,
    edge: (usize, usize),
) -> Option<&holos_tda::collapse::RemovalStep> {
    result.certificate.steps().iter().find(|s| s.edge() == edge)
}

/// Full output equality: the matrix and every certificate field, floats by
/// bits. This is the ordered path's whole contract.
fn assert_same_output(name: &str, got: &CollapsedRips, want: &CollapsedRips) {
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
fn assert_invariant_stats(name: &str, got: &CollapsedRips, want: &CollapsedRips) {
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
fn assert_work_bound(name: &str, result: &CollapsedRips) {
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
/// and only its own retirement can revoke either, so the identity gates
/// the scheduler's state invariant rather than merely bounding it.
fn assert_occupancy(name: &str, r: &CollapsedRips) {
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
fn assert_matches_serial(name: &str, ordered: &CollapsedRips, serial: &CollapsedRips) {
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

// The unpruned reference schedule, rebuilt from sections 1 and 2 of the
// specification. It shares nothing with production: a full value matrix, a
// candidate set rebuilt by scanning every vertex, an explicit critical
// value list, and a full pass over every live edge.

struct RefStep {
    edge: (usize, usize),
    value: f64,
    pass: usize,
    witnesses: Vec<(f64, usize)>,
}

struct RefRun {
    steps: Vec<RefStep>,
    survivors: Vec<(usize, usize, f64)>,
    passes: usize,
    terminal: f64,
}

/// The predicate with the witness rule, against the value matrix `f`.
/// Returns the witness segments, or `None` when some level has no
/// dominating vertex.
fn ref_test_edge(
    f: &[Vec<f64>],
    u: usize,
    v: usize,
    a: f64,
    terminal: f64,
) -> Option<Vec<(f64, usize)>> {
    let mut cands: Vec<(usize, f64)> = Vec::new();
    for (x, (&du, &dv)) in f[u].iter().zip(f[v].iter()).enumerate() {
        if x == u || x == v || !du.is_finite() || !dv.is_finite() {
            continue;
        }
        let b = a.max(du).max(dv);
        if b <= terminal {
            cands.push((x, b));
        }
    }

    let mut critical: Vec<f64> = std::iter::once(a)
        .chain(cands.iter().map(|&(_, b)| b))
        .collect();
    critical.sort_by(f64::total_cmp);
    critical.dedup();

    let mut segments: Vec<(f64, usize)> = Vec::new();
    let mut apex: Option<usize> = None;
    for t in critical {
        let level: Vec<usize> = cands
            .iter()
            .filter(|&&(_, b)| b <= t)
            .map(|&(x, _)| x)
            .collect();
        if level.is_empty() {
            return None;
        }
        let dominates = |w: usize| level.iter().all(|&x| x == w || f[w][x] <= t);
        let birth = |w: usize| cands.iter().find(|&&(x, _)| x == w).map(|&(_, b)| b);
        if let Some(w) = apex {
            if birth(w).is_some_and(|b| b <= t) && dominates(w) {
                continue;
            }
        }
        let found = level.iter().copied().find(|&w| dominates(w))?;
        segments.push((t, found));
        apex = Some(found);
    }
    Some(segments)
}

/// The frozen schedule with no pruning: every pass tests every live edge.
fn reference_collapse(n: usize, all_edges: &[(usize, usize, f64)], resolved: f64) -> RefRun {
    let mut edges: Vec<(usize, usize, f64)> = all_edges
        .iter()
        .copied()
        .filter(|&(_, _, d)| d.is_finite() && d <= resolved)
        .collect();
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        edges.iter().map(|e| e.2).fold(0.0f64, f64::max)
    };
    edges.sort_by(|a, b| b.2.total_cmp(&a.2).then((a.1, a.0).cmp(&(b.1, b.0))));

    let mut f = vec![vec![f64::INFINITY; n]; n];
    for (x, row) in f.iter_mut().enumerate() {
        row[x] = 0.0;
    }
    for &(u, v, d) in &edges {
        f[u][v] = d;
        f[v][u] = d;
    }

    let mut alive = vec![true; edges.len()];
    let mut steps: Vec<RefStep> = Vec::new();
    let mut passes = 0;
    loop {
        passes += 1;
        let mut removed_any = false;
        for i in 0..edges.len() {
            if !alive[i] {
                continue;
            }
            let (u, v, value) = edges[i];
            let Some(witnesses) = ref_test_edge(&f, u, v, value, terminal) else {
                continue;
            };
            alive[i] = false;
            f[u][v] = f64::INFINITY;
            f[v][u] = f64::INFINITY;
            steps.push(RefStep {
                edge: (u, v),
                value,
                pass: passes,
                witnesses,
            });
            removed_any = true;
        }
        if !removed_any {
            break;
        }
    }

    let mut survivors: Vec<(usize, usize, f64)> = edges
        .iter()
        .zip(&alive)
        .filter(|(_, &live)| live)
        .map(|(&e, _)| e)
        .collect();
    survivors.sort_by_key(|&(u, v, _)| (u, v));
    RefRun {
        steps,
        survivors,
        passes,
        terminal,
    }
}

fn reference_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> RefRun {
    let n = dist.len();
    let resolved = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let mut all = Vec::with_capacity(n * (n - 1) / 2);
    for u in 0..n {
        for v in (u + 1)..n {
            all.push((u, v, dist.get(u, v)));
        }
    }
    reference_collapse(n, &all, resolved)
}

fn reference_sparse(dist: &SparseDistanceMatrix, threshold: Option<f64>) -> RefRun {
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let all: Vec<(usize, usize, f64)> = dist.edges().collect();
    reference_collapse(dist.len(), &all, resolved)
}

/// Compare an ordered run against the unpruned reference, bit for bit.
fn assert_matches_reference(name: &str, result: &CollapsedRips, reference: &RefRun) {
    let steps = result.certificate.steps();
    let got: Vec<(usize, usize)> = steps.iter().map(|s| s.edge()).collect();
    let want: Vec<(usize, usize)> = reference.steps.iter().map(|s| s.edge).collect();
    assert_eq!(got, want, "{name}: removal sequence");
    for (i, (got, want)) in steps.iter().zip(&reference.steps).enumerate() {
        assert_eq!(
            got.value().to_bits(),
            want.value.to_bits(),
            "{name}: step {i} value"
        );
        assert_eq!(got.epoch(), want.pass, "{name}: step {i} pass number");
        assert_eq!(
            got.witnesses().len(),
            want.witnesses.len(),
            "{name}: step {i} segment count"
        );
        for (j, (a, b)) in got.witnesses().iter().zip(&want.witnesses).enumerate() {
            assert_eq!(
                a.0.to_bits(),
                b.0.to_bits(),
                "{name}: step {i} segment {j} start"
            );
            assert_eq!(a.1, b.1, "{name}: step {i} segment {j} apex");
        }
    }

    let output: Vec<(usize, usize, f64)> = result.matrix.edges().collect();
    assert_eq!(
        output.len(),
        reference.survivors.len(),
        "{name}: surviving edge count"
    );
    for (i, (a, b)) in output.iter().zip(&reference.survivors).enumerate() {
        assert_eq!((a.0, a.1), (b.0, b.1), "{name}: survivor {i} endpoints");
        assert_eq!(a.2.to_bits(), b.2.to_bits(), "{name}: survivor {i} value");
    }
    assert_eq!(result.stats.epochs, reference.passes, "{name}: pass count");
    assert_eq!(
        result.certificate.terminal_level().to_bits(),
        reference.terminal.to_bits(),
        "{name}: terminal level"
    );
}

/// One dense input against both references: the shipped serial run and the
/// unpruned reference.
/// `window` of `None` takes the production window through the public
/// entry point; `Some(w)` forces a window so stages cross the input.
fn assert_trace_dense(
    name: &str,
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: Option<usize>,
) -> usize {
    let ordered = match window {
        None => collapse_dense_ordered_parallel(dist, threshold, threads).unwrap(),
        Some(w) => collapse_dense_ordered_with_window(dist, threshold, threads, w).unwrap(),
    };
    let serial = collapse_dense(dist, threshold).unwrap();
    assert_matches_serial(name, &ordered, &serial);
    assert_matches_reference(name, &ordered, &reference_dense(dist, threshold));
    verify_dense(dist, threshold, &ordered)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the ordered certificate: {e}"));
    ordered.stats.removed_edges
}

fn assert_trace_sparse(
    name: &str,
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: Option<usize>,
) {
    let ordered = match window {
        None => collapse_sparse_ordered_parallel(dist, threshold, threads).unwrap(),
        Some(w) => collapse_sparse_ordered_with_window(dist, threshold, threads, w).unwrap(),
    };
    let serial = collapse_sparse(dist, threshold).unwrap();
    assert_matches_serial(name, &ordered, &serial);
    assert_matches_reference(name, &ordered, &reference_sparse(dist, threshold));
    verify_sparse(dist, threshold, &ordered)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the ordered certificate: {e}"));
}

#[test]
fn ordered_matches_serial_v1_and_reference() {
    // Small tie-heavy graphs with zeros and absent pairs, over the three
    // threshold shapes, dense and sparse.
    let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
    let mut rng = Rng::new(0x0de5_5eed_0001);
    let mut removed_total = 0usize;
    for it in 0..250 {
        let n = 2 + rng.below(11);
        let data: Vec<f64> = (0..n * (n - 1) / 2)
            .map(|_| palette[rng.below(palette.len())])
            .collect();
        let dense = DistanceMatrix::from_condensed(data).unwrap();
        let sparse = sparse_from_dense(&dense);
        let threshold = match rng.below(3) {
            0 => None,
            1 => Some(2.0),
            _ => Some(f64::INFINITY),
        };
        // Half the cases take the production window at four workers; the
        // other half force a small window and a drawn worker count, so
        // stages cross the input and the retire walk repairs across
        // stage boundaries.
        let (threads, window) = if it % 2 == 0 {
            (4, None)
        } else {
            let windows = [1, 2, 3, 5];
            let workers = [2, 4, 8];
            (workers[rng.below(3)], Some(windows[rng.below(4)]))
        };
        let name = format!(
            "random {it} (n={n} threshold={threshold:?} threads={threads} window={window:?})"
        );

        removed_total += assert_trace_dense(&name, &dense, threshold, threads, window);
        assert_trace_sparse(&name, &sparse, threshold, threads, window);
    }
    assert!(
        removed_total > 100,
        "the sweep never collapsed anything: {removed_total}"
    );

    // A 76-vertex two-value graph. Some removal here sees a common
    // neighborhood past the marking limit, so the pruning falls back to
    // retesting every live edge and the ordered run invalidates a whole
    // window remainder.
    let mut rng = Rng::new(0x0de5_5eed_0002);
    let n = 76;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if rng.uniform() < 0.95 {
                edges.push((u, v, if rng.uniform() < 0.5 { 1.0 } else { 2.0 }));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    assert_trace_dense("dense76", &dense, Some(2.0), 4, None);

    // The mixed-yield fixture: bipartite edges no schedule can touch, plus
    // a K4 that collapses.
    let dense = bipartite_k4_dense();
    assert_trace_dense("k64_64+k4 dense", &dense, None, 4, None);
    let sparse = sparse_from_dense(&dense);
    assert_trace_sparse("k64_64+k4 sparse", &sparse, None, 4, None);
}

// K64,64 with every present edge at distance 1, plus a disjoint K4. The
// bipartite block is triangle-free, so every removal must sit in the K4.
const BIP_N: usize = 128;
const K4_N: usize = 4;

fn bipartite_k4_dist(i: usize, j: usize) -> f64 {
    let across_bipartite = i < BIP_N && j < BIP_N && (i < BIP_N / 2) != (j < BIP_N / 2);
    let inside_k4 = i >= BIP_N && j >= BIP_N;
    if across_bipartite || inside_k4 {
        1.0
    } else {
        f64::INFINITY
    }
}

fn bipartite_k4_dense() -> DistanceMatrix {
    let n = BIP_N + K4_N;
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            data.push(bipartite_k4_dist(i, j));
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

/// Inputs with different pass shapes, yields, and densities.
fn invariance_inputs() -> Vec<(String, DistanceMatrix, Option<f64>)> {
    let mut rng = Rng::new(0x1ab5_e1ce_0001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    let cloud = DistanceMatrix::from_points(&points).unwrap();

    let palette = [1.0, 2.0, f64::INFINITY];
    let n = 24;
    let data: Vec<f64> = (0..n * (n - 1) / 2)
        .map(|_| palette[rng.below(palette.len())])
        .collect();
    let mixed = DistanceMatrix::from_condensed(data).unwrap();

    let mut clique = Vec::new();
    for u in 0..16 {
        for v in (u + 1)..16 {
            clique.push((u, v, 1.0));
        }
    }
    let clique = dense_from_edges(16, &clique);

    vec![
        ("cloud".to_string(), cloud, None),
        ("ties/inf".to_string(), battery_ties(), Some(f64::INFINITY)),
        ("mixed".to_string(), mixed, Some(2.0)),
        ("clique16".to_string(), clique, Some(1.0)),
        ("later_pass".to_string(), later_pass_matrix(), Some(1.0)),
        (
            "forward_arming".to_string(),
            forward_arming_matrix(),
            Some(2.0),
        ),
        ("book".to_string(), book_matrix(1.0), Some(2.0)),
    ]
}

// Two-value palette: every comparison in the predicate meets a tie.
fn battery_ties() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d002);
    let palette = [1.0, 2.0];
    let condensed: Vec<f64> = (0..6 * 5 / 2)
        .map(|_| palette[rng.below(palette.len())])
        .collect();
    DistanceMatrix::from_condensed(condensed).unwrap()
}

#[test]
fn ordered_is_invariant_across_workers_and_windows() {
    for (name, dense, threshold) in invariance_inputs() {
        let sparse = sparse_from_dense(&dense);
        let serial_dense = collapse_dense(&dense, threshold).unwrap();
        let serial_sparse = collapse_sparse(&sparse, threshold).unwrap();
        // The baseline is a real speculative run, two workers at window
        // one; zero workers would delegate to the serial implementation
        // and compare it with itself.
        let base_dense = collapse_dense_ordered_with_window(&dense, threshold, 2, 1).unwrap();
        let base_sparse = collapse_sparse_ordered_with_window(&sparse, threshold, 2, 1).unwrap();
        assert_matches_serial(
            &format!("{name}: baseline dense"),
            &base_dense,
            &serial_dense,
        );
        assert_matches_serial(
            &format!("{name}: baseline sparse"),
            &base_sparse,
            &serial_sparse,
        );

        for &workers in &WORKERS {
            // The production window enters through the entry point that
            // takes no window; the rest are forced.
            let mut runs = vec![(
                "production".to_string(),
                collapse_dense_ordered_parallel(&dense, threshold, workers).unwrap(),
                collapse_sparse_ordered_parallel(&sparse, threshold, workers).unwrap(),
            )];
            for &window in &WINDOWS {
                runs.push((
                    format!("W={window}"),
                    collapse_dense_ordered_with_window(&dense, threshold, workers, window).unwrap(),
                    collapse_sparse_ordered_with_window(&sparse, threshold, workers, window)
                        .unwrap(),
                ));
            }
            for (label, got_dense, got_sparse) in runs {
                let label = format!("{name}: workers={workers} {label}");
                // Structural fields are invariant; edge_tests,
                // max_common_neighborhood, and the scheduling counters are
                // exempt and are never compared across configurations.
                assert_same_output(&format!("{label} dense"), &got_dense, &base_dense);
                assert_invariant_stats(&format!("{label} dense"), &got_dense, &base_dense);
                assert_work_bound(&format!("{label} dense"), &got_dense);
                assert_same_output(&format!("{label} sparse"), &got_sparse, &base_sparse);
                assert_invariant_stats(&format!("{label} sparse"), &got_sparse, &base_sparse);
                assert_work_bound(&format!("{label} sparse"), &got_sparse);
            }
        }
    }
}

fn params(
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> RipsParams {
    let mut p = RipsParams::new(max_dim)
        .with_modulus(modulus)
        .with_threads(threads);
    p.threshold = threshold;
    p.use_clearing = toggles.0;
    p.use_emergent_pairs = toggles.1;
    p.use_apparent_pairs = toggles.2;
    if collapse {
        p = p.with_collapse_schedule(CollapseSchedule::Ordered);
    }
    p
}

fn canon(diagram: &Diagram) -> Vec<Bar> {
    let mut d = diagram.clone();
    d.canonicalize();
    d.bars
}

fn dense_bars(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> Vec<Bar> {
    let p = params(max_dim, threshold, modulus, threads, toggles, collapse);
    canon(&rips_persistence(dist, &p).unwrap())
}

fn sparse_bars(
    dist: &SparseDistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> Vec<Bar> {
    let p = params(max_dim, threshold, modulus, threads, toggles, collapse);
    canon(&rips_persistence_sparse(dist, &p).unwrap())
}

fn oracle_bars(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> Vec<Bar> {
    canon(&rips_persistence_oracle_mod(
        dist, max_dim, threshold, modulus,
    ))
}

/// The engine on an ordered collapsed graph, at the certificate's terminal
/// level: what a caller does by hand with the standalone entry point.
fn standalone_bars(
    collapsed: &CollapsedRips,
    max_dim: usize,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
) -> Vec<Bar> {
    let p = params(
        max_dim,
        Some(collapsed.certificate.terminal_level()),
        modulus,
        threads,
        toggles,
        false,
    );
    canon(&rips_persistence_sparse(&collapsed.matrix, &p).unwrap())
}

/// Bar-for-bar equality of the uncollapsed engine, the convenience path,
/// the standalone ordered path, and the oracle, over the modulus x
/// threshold x max_dim x reducer threads x toggles cross.
fn assert_ordered_preserves_diagram(name: &str, dense: &DistanceMatrix, mid: f64) {
    let sparse = sparse_from_dense(dense);
    for threshold in [None, Some(mid), Some(f64::INFINITY)] {
        let od = collapse_dense_ordered_parallel(dense, threshold, 4).unwrap();
        verify_dense(dense, threshold, &od)
            .unwrap_or_else(|e| panic!("{name}: verifier rejected the dense certificate: {e}"));
        let os = collapse_sparse_ordered_parallel(&sparse, threshold, 4).unwrap();
        verify_sparse(&sparse, threshold, &os)
            .unwrap_or_else(|e| panic!("{name}: verifier rejected the sparse certificate: {e}"));

        for &modulus in &MODULI {
            for max_dim in 0..=2 {
                let oracle = oracle_bars(dense, max_dim, threshold, modulus);
                for &threads in &[1usize, 4] {
                    for &toggles in &[ALL_ON, ALL_OFF] {
                        let label = format!(
                            "{name}: p={modulus} threshold={threshold:?} max_dim={max_dim} \
                             threads={threads} toggles={toggles:?}"
                        );
                        let plain =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, false);
                        let convenience =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, true);
                        assert_eq!(plain, convenience, "{label}: dense convenience path");
                        assert_eq!(
                            plain,
                            standalone_bars(&od, max_dim, modulus, threads, toggles),
                            "{label}: dense standalone ordered path"
                        );
                        assert_eq!(plain, oracle, "{label}: dense oracle");

                        let plain = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, false,
                        );
                        let convenience = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, true,
                        );
                        assert_eq!(plain, convenience, "{label}: sparse convenience path");
                        assert_eq!(
                            plain,
                            standalone_bars(&os, max_dim, modulus, threads, toggles),
                            "{label}: sparse standalone ordered path"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn ordered_preserves_the_diagram() {
    let mut rng = Rng::new(0x0d1a_6a20_0001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    let cloud = DistanceMatrix::from_points(&points).unwrap();
    assert_ordered_preserves_diagram("points", &cloud, 0.7);
    assert_ordered_preserves_diagram("ties", &battery_ties(), 1.0);

    // Coincident points and absent pairs: zero-value edges at the bottom
    // of the filtration, +inf as a missing edge.
    let sites = [[0.0, 0.0], [1.0, 0.0], [0.5, 0.9]];
    let mut points = Vec::new();
    for site in sites {
        points.push(site.to_vec());
        points.push(site.to_vec());
    }
    let zeros = DistanceMatrix::from_points(&points).unwrap();
    assert_ordered_preserves_diagram("zeros", &zeros, 0.6);

    // A gross triangle-inequality violation: domination is a graph
    // property, so the long edge still goes.
    let non_metric = dense_from_edges(
        5,
        &[
            (0, 1, 10.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 5.0),
            (1, 3, 5.0),
            (2, 3, 0.5),
            (2, 4, 3.0),
            (3, 4, 3.0),
        ],
    );
    assert_ordered_preserves_diagram("non_metric", &non_metric, 5.0);
}

/// Run one fixture through the ordered path and gate it against both
/// references, then return the ordered result for the fixture's own
/// counter checks.
fn fixture(
    name: &str,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: usize,
) -> CollapsedRips {
    let ordered = collapse_dense_ordered_with_window(dense, threshold, threads, window).unwrap();
    let serial = collapse_dense(dense, threshold).unwrap();
    assert_matches_serial(name, &ordered, &serial);
    assert_matches_reference(name, &ordered, &reference_dense(dense, threshold));
    verify_dense(dense, threshold, &ordered)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the certificate: {e}"));
    ordered
}

/// Edge (0, 1) has two candidates, 2 and 3, that are not adjacent, so no
/// apex dominates it in pass 1. Edge (0, 3) leaves in pass 1, which drops
/// candidate 3 and makes (0, 1) removable in pass 2.
fn later_pass_matrix() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 1.0),
        (1, 3, 1.0),
        (0, 4, 1.0),
        (2, 4, 1.0),
        (1, 5, 1.0),
        (2, 5, 1.0),
    ];
    dense_from_edges(6, &edges)
}

/// Component 1: edge (0, 1) is blocked by the non-adjacent candidates 2
/// and 3; vertices 4 and 5 block (1, 3) and (0, 3) through pass 1, and 6
/// and 7 block (0, 2) and (1, 2). Pass 1 removes (1, 4), (0, 5), (0, 6),
/// and (1, 7), which leaves (0, 3) due in pass 2 and (0, 1) clean.
/// Component 2 is a copy of the later-pass gadget at value 0.5 on
/// vertices 8 to 13; its (8, 9) is also due in pass 2 and sits after
/// (0, 1) in the schedule, so (0, 1) is armed inside a retirement span.
fn forward_arming_matrix() -> DistanceMatrix {
    let mut edges = vec![
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 2.0),
        (1, 3, 2.0),
        (1, 4, 2.0),
        (3, 4, 2.0),
        (0, 5, 2.0),
        (3, 5, 2.0),
        (0, 6, 1.0),
        (2, 6, 1.0),
        (1, 7, 1.0),
        (2, 7, 1.0),
    ];
    for &(u, v) in &[
        (8, 9),
        (8, 10),
        (9, 10),
        (8, 11),
        (9, 11),
        (8, 12),
        (10, 12),
        (9, 13),
        (10, 13),
    ] {
        edges.push((u, v, 0.5));
    }
    dense_from_edges(14, &edges)
}

/// Two disjoint gadgets on a shared spine (0, 1). In each gadget the
/// spoke (0, x) is blocked by the non-adjacent pair {1, z} and the spoke
/// (1, x) has the single candidate 0, so exactly (1, 2) and (1, 4) leave
/// in pass 1. The value 1.8 and 1.5 edges keep their only candidate born
/// above their own value, so they never fire in pass 1.
///
/// `spine` places the spine edge in the schedule: below 2.0 puts it last,
/// where both removals conflict with it; at 2.0 it retires first, where
/// neither does.
fn book_matrix(spine: f64) -> DistanceMatrix {
    let edges = [
        (0, 1, spine),
        (0, 2, 2.0),
        (1, 2, 2.0),
        (2, 3, 1.8),
        (0, 3, 1.5),
        (0, 4, 2.0),
        (1, 4, 2.0),
        (4, 5, 1.8),
        (0, 5, 1.5),
    ];
    dense_from_edges(6, &edges)
}

#[test]
fn forward_arming() {
    // Attack: a removal arms a previously clean edge that lies inside the
    // retirement span of the window that is running. The armed edge was
    // not a member, so its verdict cannot come from the cache; it must be
    // tested serially at its turn and leave in the same pass.
    let dense = forward_arming_matrix();
    let result = fixture("forward_arming", &dense, Some(2.0), 4, ONE_STAGE);

    let armed = step_for(&result, (0, 1)).expect("the armed edge must be removed");
    assert_eq!(
        armed.epoch(),
        2,
        "the armed edge must leave in the pass that armed it"
    );
    assert_eq!(
        armed.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the armed edge is certified by its only remaining candidate"
    );
    let arming = step_for(&result, (0, 3)).expect("the arming removal must happen");
    assert_eq!(arming.epoch(), 2, "the arming removal is in pass 2");
    assert_eq!(
        step_for(&result, (1, 4)).map(|s| s.epoch()),
        Some(1),
        "the pass 1 removal that unblocks (1, 3)"
    );
    assert!(
        result.stats.epochs >= 3,
        "a pass 2 removal needs a third pass, got {}",
        result.stats.epochs
    );

    // The same trace with no speculation at all.
    let serial_windows = collapse_dense_ordered_with_window(&dense, Some(2.0), 8, 1).unwrap();
    assert_same_output("forward_arming: W=1", &serial_windows, &result);
}

#[test]
fn backward_dirtiness() {
    // Attack: a removal dirties an already-retired lower position. The
    // schedule may not revisit it in this pass; it must come back in the
    // next one. Edge (0, 1) is tested first and fails, then the removal
    // of (0, 3) dirties it, and it leaves in pass 2.
    let dense = later_pass_matrix();
    let result = fixture("backward_dirtiness", &dense, Some(1.0), 4, ONE_STAGE);

    let step = step_for(&result, (0, 1)).expect("the dirtied edge must come back");
    assert_eq!(
        step.epoch(),
        2,
        "a backward dirty flag must be served in the next pass"
    );
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.edge() == (0, 3) && s.epoch() == 1),
        "the removal that dirties (0, 1) must be in pass 1"
    );
    assert!(
        result.stats.epochs >= 3,
        "a pass 2 removal needs a third pass, got {}",
        result.stats.epochs
    );
}

#[test]
fn stale_negative_to_positive() {
    // Attack: a cached verdict of false that becomes true before its turn.
    // At FORM, edge (0, 1) has the non-adjacent candidates 2 and 3 and no
    // apex. The earlier removal of (1, 3) drops candidate 3, so at its
    // turn the edge is dominated by vertex 2 and must leave in pass 1.
    let dense = dense_from_edges(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 1.0),
            (1, 3, 2.0),
        ],
    );
    let result = fixture(
        "stale_negative_to_positive",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    let step = step_for(&result, (0, 1)).expect("the repaired edge must be removed");
    assert_eq!(step.epoch(), 1, "the repair must happen inside pass 1");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the surviving candidate certifies the removal"
    );
    assert_eq!(
        step_for(&result, (1, 3)).map(|s| s.epoch()),
        Some(1),
        "the conflicting removal is the first step"
    );
    assert!(
        result.stats.invalidated_results >= 1,
        "a stale member must be re-evaluated at its turn"
    );
}

#[test]
fn stale_positive_to_negative() {
    // Attack: a cached verdict of true, with witnesses, that a conflicting
    // removal destroys. At FORM, edge (0, 1) is dominated by vertex 2 at
    // both levels because f(2, 3) = 2. The earlier removal of (2, 3) ends
    // that domination, and the repaired verdict must keep the edge.
    // Vertices 4 and 5 block (0, 3) and (1, 3) through pass 1.
    let dense = dense_from_edges(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 2.0),
            (1, 3, 2.0),
            (2, 3, 2.0),
            (0, 4, 2.0),
            (3, 4, 2.0),
            (1, 5, 2.0),
            (3, 5, 2.0),
        ],
    );
    let result = fixture(
        "stale_positive_to_negative",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    assert!(
        step_for(&result, (0, 1)).is_none(),
        "the stale positive must not survive the repair"
    );
    assert_eq!(
        step_for(&result, (2, 3)).map(|s| s.epoch()),
        Some(1),
        "the conflicting removal is in pass 1"
    );
    assert!(
        result.stats.invalidated_results >= 1,
        "the stale member must be re-evaluated at its turn"
    );
}

#[test]
fn changed_witness_same_verdict() {
    // Attack: a conflicting removal that changes the witnesses and leaves
    // the verdict alone. At FORM, edge (0, 1) has the candidates 2, 3, and
    // 4, and needs two segments: vertex 2 from level 1, then vertex 3 from
    // level 2, because f(2, 4) is absent. The earlier removal of (1, 4)
    // drops candidate 4, so one segment with apex 2 now covers the whole
    // range. Reusing the cache would record two segments.
    let mut edges = vec![
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 2.0),
        (1, 3, 2.0),
        (0, 4, 2.0),
        (1, 4, 2.0),
        (2, 3, 2.0),
        (3, 4, 2.0),
    ];
    // Private blockers: each keeps one value 2 edge alive through pass 1,
    // so only (1, 4) fires before (0, 1) is retired.
    for &(u, v) in &[
        (0, 5),
        (3, 5),
        (1, 6),
        (3, 6),
        (2, 7),
        (3, 7),
        (0, 8),
        (4, 8),
        (3, 9),
        (4, 9),
    ] {
        edges.push((u, v, 2.0));
    }
    let dense = dense_from_edges(10, &edges);
    let result = fixture(
        "changed_witness_same_verdict",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    let step = step_for(&result, (0, 1)).expect("the repaired edge must still be removed");
    assert_eq!(step.epoch(), 1, "the verdict is unchanged, so the pass is");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the repair must record the witnesses of the graph at the turn"
    );
    assert_eq!(
        step_for(&result, (1, 4)).map(|s| s.epoch()),
        Some(1),
        "the conflicting removal is in pass 1"
    );
    assert!(
        result.stats.invalidated_results >= 1,
        "the changed witnesses must come from a repair"
    );
}

#[test]
fn one_repair_for_many_invalidations() {
    // Attack: two removals conflict with the same cached result. The spine
    // (0, 1) is the last position, and both (1, 2) and (1, 4) mark it
    // through their affected sets {0, 1, 2} and {0, 1, 4}. Staleness is a
    // property of the slot, not a counter, so the whole run must show one
    // invalidation and one repair. No other member is ever stale: every
    // other edge of an affected set is already retired.
    let dense = book_matrix(1.0);
    let result = fixture(
        "one_repair_for_many_invalidations",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    let removals: Vec<((usize, usize), usize)> = result
        .certificate
        .steps()
        .iter()
        .map(|s| (s.edge(), s.epoch()))
        .collect();
    assert_eq!(
        removals,
        vec![((1, 2), 1), ((1, 4), 1), ((0, 2), 2), ((0, 4), 2),],
        "hand-simulated removal sequence"
    );
    assert!(
        step_for(&result, (0, 1)).is_none(),
        "the spine loses both candidates and survives"
    );
    assert_eq!(
        result.stats.invalidated_results, 1,
        "two conflicting removals may stale one slot once"
    );
    assert_eq!(
        result.stats.invalidated_results, 1,
        "one stale slot costs one repair"
    );
    assert_eq!(
        result.stats.global_invalidations, 0,
        "no affected set here reaches the marking limit"
    );
}

#[test]
fn nonconflicting_reuse() {
    // Attack: an unrelated removal between FORM and a member's turn. The
    // same two gadgets, with the spine raised to 2.0 so it retires first.
    // The removal of (1, 2) then lies between FORM and the turn of (1, 4),
    // and its affected set {0, 1, 2} holds no later member, so the cached
    // verdict of (1, 4) must be reused unchanged.
    let dense = book_matrix(2.0);
    let result = fixture("nonconflicting_reuse", &dense, Some(2.0), 4, ONE_STAGE);

    assert!(
        result.stats.removed_edges >= 2,
        "the fixture needs removals to reuse around, got {}",
        result.stats.removed_edges
    );
    assert_eq!(
        step_for(&result, (1, 2)).map(|s| s.epoch()),
        Some(1),
        "the unrelated removal is in pass 1"
    );
    assert_eq!(
        step_for(&result, (1, 4)).map(|s| s.epoch()),
        Some(1),
        "the reusing member leaves in the same pass"
    );
    assert_eq!(
        result.stats.invalidated_results, 0,
        "no removal here conflicts with a later member"
    );
    assert_eq!(
        result.stats.invalidated_results, 0,
        "nothing stale means nothing to repair"
    );
    assert_eq!(
        result.stats.edge_tests, result.stats.logical_tests,
        "with no repair, every physical test is a logical one"
    );
}

#[test]
fn large_s_global_invalidation() {
    // Attack: a removal whose affected set passes the marking limit in the
    // middle of a window. Fine marking bails, so the rest of the window
    // loses its cache and the next pass retests every live edge. K68 makes
    // the first removal bail: its affected set is all 68 vertices.
    let n = 68;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            edges.push((u, v, 1.0));
        }
    }
    let dense = dense_from_edges(n, &edges);
    let ordered = collapse_dense_ordered_with_window(&dense, Some(1.0), 4, ONE_STAGE).unwrap();
    let serial = collapse_dense(&dense, Some(1.0)).unwrap();
    assert_matches_serial("large_s_global_invalidation", &ordered, &serial);
    verify_dense(&dense, Some(1.0), &ordered)
        .unwrap_or_else(|e| panic!("large_s_global_invalidation: verifier: {e}"));

    assert!(
        ordered.stats.global_invalidations >= 1,
        "the marking limit must be reached at least once"
    );
    assert!(
        ordered.stats.invalidated_results >= 1,
        "a bail must stale the window remainder"
    );
}

#[test]
fn underfilled_final_pass() {
    // Attack: the final pass, whose due set is smaller than the window. It
    // removes nothing and must still be counted. The octahedron is a flag
    // 2-sphere with no removable edge, so its whole run is that one pass.
    let n = 6;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if u / 2 != v / 2 {
                edges.push((u, v, 1.0));
            }
        }
    }
    let octahedron = dense_from_edges(n, &edges);
    let result = fixture(
        "underfilled_final_pass",
        &octahedron,
        Some(1.0),
        8,
        ONE_STAGE,
    );
    assert!(
        result.certificate.steps().is_empty(),
        "the octahedron has no removable edge"
    );
    assert_eq!(result.stats.epochs, 1, "zero yield must take one pass");
    assert_eq!(
        result.stats.logical_tests, 12,
        "one logical test per edge in the only pass"
    );

    // A run that does remove: the last pass has no due edge at all and is
    // still counted, and no removal carries the last pass number.
    let dense = later_pass_matrix();
    let result = fixture(
        "underfilled_final_pass tail",
        &dense,
        Some(1.0),
        8,
        ONE_STAGE,
    );
    let last = result.stats.epochs;
    assert!(
        result.certificate.steps().iter().all(|s| s.epoch() < last),
        "the final pass must remove nothing"
    );
}

#[test]
fn oversized_window_and_workers() {
    // Attack: a window larger than the input and more workers than there
    // is work. Both are legal and neither may reach the output.
    let dense = later_pass_matrix();
    let base = fixture("oversized", &dense, Some(1.0), 8, 100_000);
    assert_eq!(
        base.certificate.input_edge_count(),
        9,
        "the fixture is smaller than the window"
    );
    for &workers in &[8usize, 64] {
        let got = collapse_dense_ordered_with_window(&dense, Some(1.0), workers, 100_000).unwrap();
        assert_same_output(&format!("oversized: workers={workers}"), &got, &base);
        assert_invariant_stats(&format!("oversized: workers={workers}"), &got, &base);
    }
}
