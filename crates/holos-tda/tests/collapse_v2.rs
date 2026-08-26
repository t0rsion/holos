//! Version 2 collapse gates: the snapshot-round schedule.
//!
//! The version 2 collapser tests every live edge against a frozen snapshot
//! of the graph, orders the successes by the frozen priority, takes a greedy
//! batch of pairwise non-conflicting edges, and deletes the batch. The gates
//! here pin that schedule from three sides: an independent unpruned
//! reference written from the specification, byte-identical output at every
//! worker count, and bar-for-bar equality with the uncollapsed engine, the
//! version 1 schedule, and the brute-force oracle.
//!
//! The version 2 output is not the version 1 output. Neither graph is
//! canonical; only the barcode is.
//!
//! The named fixtures below each say which part of the round structure they
//! attack. Their expected certificates are derived from the specification,
//! not observed from a run.

use holos_tda::collapse::verify::{verify_dense, verify_sparse};
use holos_tda::collapse::{
    collapse_dense, collapse_dense_rounds_parallel, collapse_sparse,
    collapse_sparse_rounds_parallel, CollapsedRips, RemovalStep,
};
use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{
    rips_persistence, rips_persistence_sparse, Bar, CollapseSchedule, Diagram, DistanceMatrix,
    RipsParams, SparseDistanceMatrix,
};

const MODULI: [u32; 3] = [2, 3, 5];
const COLLAPSE_THREADS: [usize; 4] = [1, 2, 4, 8];
const REDUCER_THREADS: [usize; 2] = [1, 4];
const ALL_ON: (bool, bool, bool) = (true, true, true);
const ALL_OFF: (bool, bool, bool) = (false, false, false);
/// The production marking limit. Above this many vertices in the removed
/// edge's closed common neighborhood, the collapser drops fine marking and
/// retests every live edge in the next round.
const MARK_LIMIT: usize = 64;

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
        p = p.with_collapse_schedule(CollapseSchedule::Rounds);
    }
    p
}

type StepBits = (usize, usize, u64, usize, Vec<(u64, usize)>);

/// Certificate steps with every float taken by bits, so `-0.0` and `0.0`
/// do not compare equal.
fn step_bits(cert: &holos_tda::collapse::CollapseCertificate) -> Vec<StepBits> {
    cert.steps()
        .iter()
        .map(|s| {
            let (u, v) = s.edge();
            (
                u,
                v,
                s.value().to_bits(),
                s.epoch(),
                s.witnesses()
                    .iter()
                    .map(|&(a, w)| (a.to_bits(), w))
                    .collect(),
            )
        })
        .collect()
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

/// The engine on a collapsed graph, at the certificate's terminal level:
/// what a caller does by hand with a standalone collapse.
fn collapsed_bars(
    result: &CollapsedRips,
    max_dim: usize,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
) -> Vec<Bar> {
    let inner = params(
        max_dim,
        Some(result.certificate.terminal_level()),
        modulus,
        threads,
        toggles,
        false,
    );
    canon(&rips_persistence_sparse(&result.matrix, &inner).unwrap())
}

fn essential_count(bars: &[Bar], dim: usize) -> usize {
    bars.iter()
        .filter(|b| b.dim == dim && b.death.is_infinite())
        .count()
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

fn edge_list(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, f64)> {
    matrix.edges().collect()
}

/// The edges the collapse must describe: finite, at or below the resolved
/// threshold, in ascending endpoint order.
fn thresholded_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> Vec<(usize, usize, f64)> {
    let t = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let n = dist.len();
    let mut out = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            let d = dist.get(u, v);
            if d.is_finite() && d <= t {
                out.push((u, v, d));
            }
        }
    }
    out
}

fn step_for(result: &CollapsedRips, edge: (usize, usize)) -> Option<&RemovalStep> {
    result.certificate.steps().iter().find(|s| s.edge() == edge)
}

fn removed_set(result: &CollapsedRips) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = result
        .certificate
        .steps()
        .iter()
        .map(|s| s.edge())
        .collect();
    v.sort_unstable();
    v
}

/// Steps per round, one entry per schedule epoch. The last entry is the
/// closing round, which removes nothing.
fn round_widths(result: &CollapsedRips) -> Vec<usize> {
    let mut widths = vec![0usize; result.stats.epochs];
    for step in result.certificate.steps() {
        let round = step.epoch();
        assert!(
            round >= 1 && round <= widths.len(),
            "step round {round} outside the {} recorded epochs",
            widths.len()
        );
        widths[round - 1] += 1;
    }
    widths
}

fn adjacency(n: usize, edges: &[(usize, usize, f64)]) -> Vec<Vec<bool>> {
    let mut adj = vec![vec![false; n]; n];
    for &(u, v, _) in edges {
        adj[u][v] = true;
        adj[v][u] = true;
    }
    adj
}

/// The closed common neighborhood S(e) = N[u] intersect N[v], sorted.
fn common_closed_set(adj: &[Vec<bool>], u: usize, v: usize) -> Vec<usize> {
    let n = adj.len();
    let mut s: Vec<usize> = (0..n)
        .filter(|&x| x != u && x != v && adj[u][x] && adj[v][x])
        .collect();
    s.push(u);
    s.push(v);
    s.sort_unstable();
    s
}

/// Largest |S(e)| seen at a removal, measured on the snapshot its round
/// read. Replayed from the certificate alone, independently of any
/// production counter: rounds group by schedule epoch, and every step of a
/// round reads the graph as it stood before the round.
fn widest_round_read_set(n: usize, input: &[(usize, usize, f64)], result: &CollapsedRips) -> usize {
    let mut adj = adjacency(n, input);
    let steps = result.certificate.steps();
    let mut widest = 0;
    let mut i = 0;
    while i < steps.len() {
        let round = steps[i].epoch();
        let mut j = i;
        while j < steps.len() && steps[j].epoch() == round {
            j += 1;
        }
        for step in &steps[i..j] {
            let (u, v) = step.edge();
            widest = widest.max(common_closed_set(&adj, u, v).len());
        }
        for step in &steps[i..j] {
            let (u, v) = step.edge();
            adj[u][v] = false;
            adj[v][u] = false;
        }
        i = j;
    }
    widest
}

/// Run the version 2 collapser on a dense input and hand the certificate to
/// the independent verifier. Every version 2 certificate in this file goes
/// through here or through [`v2_sparse`].
fn v2_dense(
    name: &str,
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> CollapsedRips {
    let result = collapse_dense_rounds_parallel(dist, threshold, threads).unwrap();
    assert_eq!(
        result.certificate.algorithm_version(),
        2,
        "{name}: algorithm version"
    );
    verify_dense(dist, threshold, &result)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the version 2 certificate: {e}"));
    result
}

fn v2_sparse(
    name: &str,
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> CollapsedRips {
    let result = collapse_sparse_rounds_parallel(dist, threshold, threads).unwrap();
    assert_eq!(
        result.certificate.algorithm_version(),
        2,
        "{name}: algorithm version"
    );
    verify_sparse(dist, threshold, &result)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the version 2 certificate: {e}"));
    result
}

// Production may skip an edge whose verdict provably cannot have changed
// since its last test, and falls back to retesting everything once the
// affected vertex set grows past the marking limit. Only the test counter
// may move: the removal sequence, the round numbers, the witnesses, and the
// surviving graph must equal what a collapser that retests every live edge
// every round produces.
//
// The reference below is written from sections 1 and 2 of the version 2
// specification and the unchanged predicate of the version 1 specification.
// It shares nothing with production: a full value matrix instead of sorted
// adjacency lists with tombstones, a candidate set rebuilt by scanning all
// vertices, an explicit critical-value list, a cloned snapshot per round, a
// full test of every live edge, and a conflict check against every earlier
// selection of the round.

struct RefStep {
    edge: (usize, usize),
    value: f64,
    epoch: usize,
    witnesses: Vec<(f64, usize)>,
}

/// One live edge that passed the predicate against the round's snapshot.
struct RefSuccess {
    u: usize,
    v: usize,
    value: f64,
    witnesses: Vec<(f64, usize)>,
}

/// Everything the certificate and the output matrix record, as produced by
/// the reference schedule.
struct RefRun {
    steps: Vec<RefStep>,
    survivors: Vec<(usize, usize, f64)>,
    epochs: usize,
    terminal: f64,
}

/// The version 1 section 2 predicate with the section 3 witness rule,
/// evaluated against the value matrix `f`. Returns the witness segments, or
/// `None` when some level has no dominating vertex. The version 2 schedule
/// leaves this rule untouched; only the graph it reads changes.
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
        // C_t in increasing vertex order, as the witness rule requires.
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

/// Run the version 2 schedule with no pruning: every round tests every live
/// edge against a frozen snapshot, sorts the successes by value descending
/// with ties by ascending (v, u), then takes them greedily while no earlier
/// selection of the round has both endpoints of the candidate in its closed
/// common neighborhood.
fn reference_collapse_v2(n: usize, all_edges: &[(usize, usize, f64)], resolved: f64) -> RefRun {
    let edges: Vec<(usize, usize, f64)> = all_edges
        .iter()
        .copied()
        .filter(|&(_, _, d)| d.is_finite() && d <= resolved)
        .collect();
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        edges.iter().map(|e| e.2).fold(0.0f64, f64::max)
    };

    let mut f = vec![vec![f64::INFINITY; n]; n];
    for (x, row) in f.iter_mut().enumerate() {
        row[x] = 0.0;
    }
    for &(u, v, d) in &edges {
        f[u][v] = d;
        f[v][u] = d;
    }

    let mut live = edges.clone();
    let mut steps: Vec<RefStep> = Vec::new();
    let mut epochs = 0;
    loop {
        epochs += 1;
        let snapshot = f.clone();

        let mut successes: Vec<RefSuccess> = live
            .iter()
            .filter_map(|&(u, v, value)| {
                ref_test_edge(&snapshot, u, v, value, terminal).map(|witnesses| RefSuccess {
                    u,
                    v,
                    value,
                    witnesses,
                })
            })
            .collect();
        if successes.is_empty() {
            break;
        }
        successes.sort_by(|a, b| {
            b.value
                .total_cmp(&a.value)
                .then((a.v, a.u).cmp(&(b.v, b.u)))
        });

        let mut read_sets: Vec<Vec<bool>> = Vec::new();
        let mut batch: Vec<&RefSuccess> = Vec::new();
        for success in &successes {
            let (u, v) = (success.u, success.v);
            if read_sets.iter().any(|s| s[u] && s[v]) {
                continue;
            }
            let mut s = vec![false; n];
            for (x, flag) in s.iter_mut().enumerate() {
                *flag =
                    x == u || x == v || (snapshot[u][x].is_finite() && snapshot[v][x].is_finite());
            }
            read_sets.push(s);
            batch.push(success);
        }

        for success in &batch {
            steps.push(RefStep {
                edge: (success.u, success.v),
                value: success.value,
                epoch: epochs,
                witnesses: success.witnesses.clone(),
            });
        }
        for success in &batch {
            f[success.u][success.v] = f64::INFINITY;
            f[success.v][success.u] = f64::INFINITY;
        }
        live.retain(|&(u, v, _)| f[u][v].is_finite());
    }

    let mut survivors = live;
    survivors.sort_by_key(|&(u, v, _)| (u, v));
    RefRun {
        steps,
        survivors,
        epochs,
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
    reference_collapse_v2(n, &all, resolved)
}

fn reference_sparse(dist: &SparseDistanceMatrix, threshold: Option<f64>) -> RefRun {
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let all: Vec<(usize, usize, f64)> = dist.edges().collect();
    reference_collapse_v2(dist.len(), &all, resolved)
}

/// Compare a production run against the reference, bit for bit.
fn assert_reference_match(name: &str, result: &CollapsedRips, reference: &RefRun) {
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
        assert_eq!(got.epoch(), want.epoch, "{name}: step {i} round number");
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

    let output = edge_list(&result.matrix);
    assert_eq!(
        output.len(),
        reference.survivors.len(),
        "{name}: surviving edge count"
    );
    for (i, (a, b)) in output.iter().zip(&reference.survivors).enumerate() {
        assert_eq!((a.0, a.1), (b.0, b.1), "{name}: survivor {i} endpoints");
        assert_eq!(a.2.to_bits(), b.2.to_bits(), "{name}: survivor {i} value");
    }
    assert_eq!(result.stats.epochs, reference.epochs, "{name}: round count");
    assert_eq!(
        result.certificate.terminal_level().to_bits(),
        reference.terminal.to_bits(),
        "{name}: terminal level"
    );
}

/// The complete graph on `n` vertices, every edge at distance 1.
fn complete_matrix(n: usize) -> DistanceMatrix {
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            edges.push((u, v, 1.0));
        }
    }
    dense_from_edges(n, &edges)
}

/// `blocks` disjoint copies of K4, every edge at distance 1.
fn disjoint_k4_matrix(blocks: usize) -> DistanceMatrix {
    let n = 4 * blocks;
    let mut edges = Vec::new();
    for b in 0..blocks {
        let base = 4 * b;
        for u in 0..4 {
            for v in (u + 1)..4 {
                edges.push((base + u, base + v, 1.0));
            }
        }
    }
    dense_from_edges(n, &edges)
}

/// K4 minus the pair (0, 3): two triangles sharing the edge (1, 2).
fn diamond_matrix() -> DistanceMatrix {
    dense_from_edges(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (1, 3, 1.0),
            (2, 3, 1.0),
        ],
    )
}

/// Edge (0, 1) has two candidates, 2 and 3, that are not adjacent, so no
/// apex dominates it in round 1. Vertices 4 and 5 block the two edges that
/// would otherwise dissolve candidate 2. Round 1 drops (0, 3), which drops
/// candidate 3 and makes (0, 1) removable in round 2.
fn later_round_matrix() -> DistanceMatrix {
    dense_from_edges(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 1.0),
            (1, 3, 1.0),
            (0, 4, 1.0),
            (2, 4, 1.0),
            (1, 5, 1.0),
            (2, 5, 1.0),
        ],
    )
}

/// Vertices 0 and 1 share every other vertex as a common neighbor, and
/// vertex 2 is adjacent to all of them, so (0, 1) is removable and its
/// closed common neighborhood is the whole graph. With 76 vertices that set
/// is far past the marking limit, so production must drop fine marking and
/// retest everything in the next round. The leaves carry no edges among
/// themselves, so the graph still reaches its fixed point in three rounds.
const FALLBACK_N: usize = 76;

fn fallback_matrix() -> DistanceMatrix {
    let mut edges = vec![(0, 1, 1.0)];
    for x in 2..FALLBACK_N {
        edges.push((0, x, 1.0));
        edges.push((1, x, 1.0));
    }
    for x in 3..FALLBACK_N {
        edges.push((2, x, 1.0));
    }
    dense_from_edges(FALLBACK_N, &edges)
}

// K64,64 with every present edge at distance 1. The bipartite graph is
// triangle-free, so no edge has a candidate and the collapse must return the
// input untouched. The barcode is known: one component and
// b1 = 4096 - 128 + 1 = 3969 essential H1 classes.
const BIP_N: usize = 128;
const K4_N: usize = 4;

fn bipartite_dense() -> DistanceMatrix {
    let mut data = Vec::with_capacity(BIP_N * (BIP_N - 1) / 2);
    for i in 1..BIP_N {
        for j in 0..i {
            let across = (i < BIP_N / 2) != (j < BIP_N / 2);
            data.push(if across { 1.0 } else { f64::INFINITY });
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

// The same bipartite block plus a disjoint K4. The K4 is the only place
// where a removal can happen, so it separates the two halves of the
// schedule.
fn bipartite_k4_dense() -> DistanceMatrix {
    let n = BIP_N + K4_N;
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            let across = i < BIP_N && j < BIP_N && (i < BIP_N / 2) != (j < BIP_N / 2);
            let inside_k4 = i >= BIP_N && j >= BIP_N;
            data.push(if across || inside_k4 {
                1.0
            } else {
                f64::INFINITY
            });
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

/// L1 distances on a 4x4 grid: only the integers 1 through 6, so almost
/// every candidate shares a birth level with its neighbors.
fn tie_heavy_grid() -> DistanceMatrix {
    let side = 4i64;
    let coords: Vec<(i64, i64)> = (0..side)
        .flat_map(|x| (0..side).map(move |y| (x, y)))
        .collect();
    let mut condensed = Vec::new();
    for i in 1..coords.len() {
        for j in 0..i {
            let d = (coords[i].0 - coords[j].0).abs() + (coords[i].1 - coords[j].1).abs();
            condensed.push(d as f64);
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

/// A seeded random graph on two values plus absent pairs.
fn random_matrix(seed: u64, n: usize, density: f64) -> DistanceMatrix {
    let mut rng = Rng::new(seed);
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if rng.uniform() < density {
                edges.push((u, v, if rng.uniform() < 0.5 { 1.0 } else { 2.0 }));
            }
        }
    }
    dense_from_edges(n, &edges)
}

// Generic point cloud: distinct values, no ties, no absent edges.
fn battery_points() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    DistanceMatrix::from_points(&points).unwrap()
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
fn production_matches_unpruned_v2_reference() {
    // Small tie-heavy graphs: zeros, repeated values, and absent pairs, over
    // the three threshold shapes, dense and sparse, at one and four workers.
    let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
    let mut rng = Rng::new(0x2ec0_11a9_5e02);
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
        let dense_reference = reference_dense(&dense, threshold);
        let sparse_reference = reference_sparse(&sparse, threshold);
        for &threads in &[1usize, 4] {
            let name = format!("random {it} (n={n} threshold={threshold:?} threads={threads})");
            let result = v2_dense(&name, &dense, threshold, threads);
            assert_reference_match(&name, &result, &dense_reference);
            removed_total += result.stats.removed_edges;
            let result = v2_sparse(&name, &sparse, threshold, threads);
            assert_reference_match(&name, &result, &sparse_reference);
        }
    }
    assert!(
        removed_total > 100,
        "the sweep never collapsed anything: {removed_total}"
    );

    // A denser seeded graph on two values: wider candidate sets, more
    // conflicts, and many rounds.
    let dense = random_matrix(0x2ec0_11a9_5e03, 24, 0.7);
    let threshold = Some(2.0);
    let reference = reference_dense(&dense, threshold);
    for &threads in &[1usize, 4] {
        let name = format!("random24 (threads={threads})");
        let result = v2_dense(&name, &dense, threshold, threads);
        assert_reference_match(&name, &result, &reference);
        assert!(result.stats.removed_edges > 0, "{name}: no removal");
    }

    // The marking fallback: the first removal reads a closed common
    // neighborhood of every vertex in the graph, far past the marking limit,
    // so production retests everything in the next round. The reference
    // never prunes, so this is the gate on that fallback.
    let dense = fallback_matrix();
    let threshold = Some(1.0);
    let reference = reference_dense(&dense, threshold);
    let result = v2_dense("fallback", &dense, threshold, 4);
    assert_reference_match("fallback", &result, &reference);
    let widest = widest_round_read_set(FALLBACK_N, &thresholded_dense(&dense, threshold), &result);
    assert!(
        widest > MARK_LIMIT,
        "fallback: widest read set is {widest}, too small to force the retest fallback"
    );

    // The mixed-yield fixture: 4,096 bipartite edges that no schedule can
    // touch, plus a K4 that collapses.
    let dense = bipartite_k4_dense();
    let sparse = sparse_from_dense(&dense);
    let dense_reference = reference_dense(&dense, None);
    let sparse_reference = reference_sparse(&sparse, None);
    for &threads in &[1usize, 4] {
        let name = format!("k64_64+k4 (threads={threads})");
        let result = v2_dense(&name, &dense, None, threads);
        assert_reference_match(&name, &result, &dense_reference);
        let result = v2_sparse(&name, &sparse, None, threads);
        assert_reference_match(&name, &result, &sparse_reference);
    }
}

/// Every worker count must give the same certificate, the same matrix, and
/// the same counters, `edge_tests` included: the pruning rule is a function
/// of the schedule, not of the worker count.
fn assert_thread_invariant(name: &str, dense: &DistanceMatrix, threshold: Option<f64>) {
    let sparse = sparse_from_dense(dense);
    let base_dense = v2_dense(name, dense, threshold, COLLAPSE_THREADS[0]);
    let base_sparse = v2_sparse(name, &sparse, threshold, COLLAPSE_THREADS[0]);
    for &threads in &COLLAPSE_THREADS[1..] {
        let got = v2_dense(name, dense, threshold, threads);
        assert_eq!(
            got.certificate, base_dense.certificate,
            "{name}: dense certificate changed at {threads} workers"
        );
        assert_eq!(
            step_bits(&got.certificate),
            step_bits(&base_dense.certificate),
            "{name}: dense certificate bits changed at {threads} workers"
        );
        assert_eq!(
            edge_list(&got.matrix),
            edge_list(&base_dense.matrix),
            "{name}: dense matrix changed at {threads} workers"
        );
        assert_eq!(
            got.stats, base_dense.stats,
            "{name}: dense stats changed at {threads} workers"
        );

        let got = v2_sparse(name, &sparse, threshold, threads);
        assert_eq!(
            got.certificate, base_sparse.certificate,
            "{name}: sparse certificate changed at {threads} workers"
        );
        assert_eq!(
            step_bits(&got.certificate),
            step_bits(&base_sparse.certificate),
            "{name}: sparse certificate bits changed at {threads} workers"
        );
        assert_eq!(
            edge_list(&got.matrix),
            edge_list(&base_sparse.matrix),
            "{name}: sparse matrix changed at {threads} workers"
        );
        assert_eq!(
            got.stats, base_sparse.stats,
            "{name}: sparse stats changed at {threads} workers"
        );
    }
}

#[test]
fn v2_is_thread_invariant() {
    assert_thread_invariant("tie_heavy_grid", &tie_heavy_grid(), None);
    assert_thread_invariant("tie_heavy_grid t=3", &tie_heavy_grid(), Some(3.0));
    assert_thread_invariant("random16", &random_matrix(0x7a1e_0001, 16, 0.6), Some(2.0));
    assert_thread_invariant("random24", &random_matrix(0x7a1e_0002, 24, 0.4), None);
    assert_thread_invariant("k5", &complete_matrix(5), Some(1.0));
    assert_thread_invariant("k4x8", &disjoint_k4_matrix(8), Some(1.0));
    assert_thread_invariant("fallback", &fallback_matrix(), Some(1.0));
    assert_thread_invariant("k64_64+k4", &bipartite_k4_dense(), None);
}

/// Bar-for-bar equality of five paths: the uncollapsed engine, the
/// convenience path, the standalone version 1 collapse, the standalone
/// version 2 collapse, and the oracle. The schedules differ; the barcode
/// does not.
fn assert_v2_preserves_the_diagram(
    name: &str,
    dense: &DistanceMatrix,
    mid: f64,
    check_oracle: bool,
) {
    let sparse = sparse_from_dense(dense);
    for threshold in [None, Some(mid), Some(f64::INFINITY)] {
        // The collapse depends on the input and the threshold only, so one
        // run of each schedule serves the whole cross below.
        let v1_dense = collapse_dense(dense, threshold).unwrap();
        let v1_sparse = collapse_sparse(&sparse, threshold).unwrap();
        let v2d = v2_dense(name, dense, threshold, 4);
        let v2s = v2_sparse(name, &sparse, threshold, 4);
        for &modulus in &MODULI {
            for max_dim in 0..=2 {
                for &threads in &REDUCER_THREADS {
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
                            collapsed_bars(&v1_dense, max_dim, modulus, threads, toggles),
                            "{label}: dense standalone version 1"
                        );
                        assert_eq!(
                            plain,
                            collapsed_bars(&v2d, max_dim, modulus, threads, toggles),
                            "{label}: dense standalone version 2"
                        );

                        let plain = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, false,
                        );
                        let convenience = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, true,
                        );
                        assert_eq!(plain, convenience, "{label}: sparse convenience path");
                        assert_eq!(
                            plain,
                            collapsed_bars(&v1_sparse, max_dim, modulus, threads, toggles),
                            "{label}: sparse standalone version 1"
                        );
                        assert_eq!(
                            plain,
                            collapsed_bars(&v2s, max_dim, modulus, threads, toggles),
                            "{label}: sparse standalone version 2"
                        );
                    }
                }
            }
            if check_oracle {
                assert_eq!(
                    collapsed_bars(&v2d, 2, modulus, 1, ALL_ON),
                    oracle_bars(dense, 2, threshold, modulus),
                    "{name}: version 2 diagram differs from the oracle \
                     (p={modulus} threshold={threshold:?})"
                );
            }
        }
    }
}

#[test]
fn v2_preserves_the_diagram() {
    assert_v2_preserves_the_diagram("points", &battery_points(), 0.7, true);
    assert_v2_preserves_the_diagram("ties", &battery_ties(), 1.0, true);
    assert_v2_preserves_the_diagram("diamond", &diamond_matrix(), 1.0, true);
}

/// Collapse on against collapse off at a few fields, dense and sparse. The
/// fixtures below use this to keep their round-structure claims tied to a
/// preserved barcode.
fn assert_fixture_barcode(
    name: &str,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    max_dim: usize,
) {
    let sparse = sparse_from_dense(dense);
    for &modulus in &MODULI {
        let plain = dense_bars(dense, max_dim, threshold, modulus, 1, ALL_ON, false);
        let collapsed = dense_bars(dense, max_dim, threshold, modulus, 2, ALL_ON, true);
        assert_eq!(
            plain, collapsed,
            "{name}: dense collapse changed the diagram (p={modulus})"
        );
        let plain = sparse_bars(&sparse, max_dim, threshold, modulus, 1, ALL_ON, false);
        let collapsed = sparse_bars(&sparse, max_dim, threshold, modulus, 2, ALL_ON, true);
        assert_eq!(
            plain, collapsed,
            "{name}: sparse collapse changed the diagram (p={modulus})"
        );
    }
}

#[test]
fn overlapping_but_commuting_read_sets() {
    // Two triangles sharing vertex 2. The read sets S((0, 1)) = {0, 1, 2}
    // and S((2, 3)) = {2, 3, 4} share a vertex, but neither edge has both
    // endpoints inside the other set, so the two removals commute and the
    // conflict rule must keep them in the same round. A rule that blocked on
    // any read-set overlap would split them.
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (2, 4, 1.0),
        (3, 4, 1.0),
    ];
    let dense = dense_from_edges(5, &edges);
    let adj = adjacency(5, &edges);
    let s01 = common_closed_set(&adj, 0, 1);
    let s23 = common_closed_set(&adj, 2, 3);
    assert_eq!(s01, vec![0, 1, 2], "S((0, 1))");
    assert_eq!(s23, vec![2, 3, 4], "S((2, 3))");
    assert!(
        s01.contains(&2) && s23.contains(&2),
        "the two read sets must share a vertex"
    );
    assert!(
        !(s01.contains(&2) && s01.contains(&3)),
        "edge (2, 3) must not lie inside S((0, 1))"
    );
    assert!(
        !(s23.contains(&0) && s23.contains(&1)),
        "edge (0, 1) must not lie inside S((2, 3))"
    );

    let result = v2_dense("bowtie", &dense, Some(1.0), 2);
    let first = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable");
    let second = step_for(&result, (2, 3)).expect("edge (2, 3) must be removable");
    assert_eq!(first.epoch(), 1, "edge (0, 1) must go in round 1");
    assert_eq!(second.epoch(), 1, "edge (2, 3) must go in round 1");
    assert_eq!(
        round_widths(&result)[0],
        2,
        "round 1 must take both commuting removals"
    );
    assert_fixture_barcode("bowtie", &dense, Some(1.0), 2);
}

#[test]
fn conflicting_removals_split_rounds() {
    // K4 alone. In the complete graph S(e) is every vertex, so every pair of
    // removable edges conflicts and round 1 can take one edge only. The rest
    // must wait for later rounds, and the schedule must stop at a spanning
    // star, where no edge has a common neighbor.
    let dense = complete_matrix(4);
    let result = v2_dense("k4", &dense, Some(1.0), 2);
    assert_eq!(
        round_widths(&result),
        vec![1, 2, 0],
        "K4: one removal in the conflict clique, then the two that commute"
    );
    assert_eq!(
        result.certificate.steps().len(),
        3,
        "K4 collapses to a spanning tree"
    );
    assert_eq!(result.stats.epochs, 3, "K4: rounds");
    assert!(
        result.certificate.steps().iter().any(|s| s.epoch() >= 2),
        "the conflicting removals must spread over rounds"
    );

    let survivors = edge_list(&result.matrix);
    assert_eq!(survivors.len(), 3, "three edges survive");
    let hub = (0..4)
        .find(|&h| survivors.iter().all(|&(u, v, _)| u == h || v == h))
        .expect("the fixed point must be a spanning star");
    assert!(hub < 4, "star centre out of range");
    assert_fixture_barcode("k4", &dense, Some(1.0), 2);
}

#[test]
fn conflict_clique_batch_width_one() {
    // A complete graph is one conflict clique: S(e) is every vertex, so the
    // greedy selection can take a single edge however many are removable.
    // K3 stays complete until it stops yielding, so every one of its rounds
    // has width one; the larger cliques must at least start that way.
    for n in 3..=6 {
        let dense = complete_matrix(n);
        let result = v2_dense(&format!("k{n}"), &dense, Some(1.0), 2);
        let widths = round_widths(&result);
        assert_eq!(
            widths[0], 1,
            "K{n}: the conflict clique allows one removal in round 1, got {widths:?}"
        );
    }

    let dense = complete_matrix(3);
    let result = v2_dense("k3", &dense, Some(1.0), 1);
    assert_eq!(
        round_widths(&result),
        vec![1, 0],
        "K3: one removal, then a round that removes nothing"
    );
    assert_eq!(
        result.stats.epochs,
        result.certificate.steps().len() + 1,
        "K3: one round per removal plus the closing round"
    );
    assert_fixture_barcode("k3", &dense, Some(1.0), 2);
}

#[test]
fn many_disjoint_k4s() {
    // Eight K4 components. Conflicts never cross a component, so the batch
    // width is the component count: round 1 must take exactly one edge from
    // each K4, which is what makes the round structure scale at all.
    let blocks = 8;
    let dense = disjoint_k4_matrix(blocks);
    let result = v2_dense("k4x8", &dense, Some(1.0), 4);
    assert_eq!(
        round_widths(&result),
        vec![blocks, 2 * blocks, 0],
        "eight independent K4 schedules must run in lockstep"
    );
    assert_eq!(
        result.certificate.steps().len(),
        3 * blocks,
        "each K4 gives up three edges"
    );

    let mut round1: Vec<usize> = result
        .certificate
        .steps()
        .iter()
        .filter(|s| s.epoch() == 1)
        .map(|s| s.edge().0 / 4)
        .collect();
    round1.sort_unstable();
    assert_eq!(
        round1,
        (0..blocks).collect::<Vec<_>>(),
        "round 1 must take one edge from every component"
    );
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert_eq!(u / 4, v / 4, "removal of ({u}, {v}) crossed a component");
    }
    assert_fixture_barcode("k4x8", &dense, Some(1.0), 2);
}

#[test]
fn later_round_removability() {
    // Edge (0, 1) has two candidates, 2 and 3, and they are not adjacent, so
    // it fails against the round 1 snapshot. Round 1 removes (0, 3), which
    // drops candidate 3, and (0, 1) leaves in round 2. A schedule that only
    // ever read the first snapshot would keep it forever.
    let dense = later_round_matrix();
    let result = v2_dense("later_round", &dense, Some(1.0), 2);
    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable in a later round");
    assert_eq!(
        step.epoch(),
        2,
        "edge (0, 1) must survive round 1 and leave in round 2"
    );
    assert!(
        result.certificate.steps().iter().any(|s| s.epoch() >= 2),
        "no removal happened after the first round"
    );
    assert_eq!(
        round_widths(&result),
        vec![3, 1, 0],
        "three commuting removals, then the edge they unlocked"
    );
    assert!(
        result.stats.epochs >= 3,
        "a removal in round 2 needs a third, empty round, got {}",
        result.stats.epochs
    );
    assert_fixture_barcode("later_round", &dense, Some(1.0), 2);
}

#[test]
fn v1_v2_schedules_diverge() {
    // Two triangles sharing the edge (1, 2), with the pair (0, 3) absent.
    // Version 1 removes (0, 1) and then sees a graph where (1, 2) has become
    // removable, so it takes that. Version 2 tests everything against the
    // round 1 snapshot, where (1, 2) fails and (1, 3) succeeds, and (1, 3)
    // does not conflict with (0, 1). The two schedules therefore delete
    // different edges and stop at different fixed points. Only the barcode
    // has to agree.
    let dense = diamond_matrix();
    let sparse = sparse_from_dense(&dense);
    let threshold = Some(1.0);
    let v1 = collapse_dense(&dense, threshold).unwrap();
    let v2 = v2_dense("diamond", &dense, threshold, 2);

    assert_eq!(v1.certificate.algorithm_version(), 1, "version 1 tag");
    assert_eq!(v2.certificate.algorithm_version(), 2, "version 2 tag");
    assert_ne!(
        removed_set(&v1),
        removed_set(&v2),
        "the two schedules must delete different edge sets here"
    );
    assert_ne!(
        v1.certificate, v2.certificate,
        "the certificates must differ"
    );
    assert_ne!(
        edge_list(&v1.matrix),
        edge_list(&v2.matrix),
        "the two fixed points must differ"
    );
    assert_eq!(
        v1.certificate.steps().len(),
        v2.certificate.steps().len(),
        "both schedules remove two edges here"
    );

    let v1_sparse = collapse_sparse(&sparse, threshold).unwrap();
    let v2_sparse_result = v2_sparse("diamond", &sparse, threshold, 2);
    for &modulus in &MODULI {
        for max_dim in 0..=2 {
            let plain = dense_bars(&dense, max_dim, threshold, modulus, 1, ALL_ON, false);
            let label = format!("diamond p={modulus} max_dim={max_dim}");
            assert_eq!(
                plain,
                collapsed_bars(&v1, max_dim, modulus, 1, ALL_ON),
                "{label}: version 1 dense"
            );
            assert_eq!(
                plain,
                collapsed_bars(&v2, max_dim, modulus, 1, ALL_ON),
                "{label}: version 2 dense"
            );
            assert_eq!(
                plain,
                collapsed_bars(&v1_sparse, max_dim, modulus, 1, ALL_ON),
                "{label}: version 1 sparse"
            );
            assert_eq!(
                plain,
                collapsed_bars(&v2_sparse_result, max_dim, modulus, 1, ALL_ON),
                "{label}: version 2 sparse"
            );
            assert_eq!(
                plain,
                oracle_bars(&dense, max_dim, threshold, modulus),
                "{label}: oracle"
            );
        }
    }
}

#[test]
fn large_neighborhood_fallback() {
    // Vertices 0 and 1 share every other vertex, and vertex 2 dominates all
    // of them, so (0, 1) goes first and its read set is the whole 76-vertex
    // graph. That is far past the marking limit, so production must abandon
    // fine marking and retest every live edge in round 2. The certificate
    // may not notice: only the test counter moves.
    let dense = fallback_matrix();
    let threshold = Some(1.0);
    let input = thresholded_dense(&dense, threshold);
    let result = v2_dense("fallback", &dense, threshold, 4);

    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable");
    assert_eq!(step.epoch(), 1, "edge (0, 1) leads the schedule");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the hub is the first dominating vertex at the only critical value"
    );
    assert_eq!(
        round_widths(&result)[0],
        1,
        "the read set covers the graph, so round 1 takes one edge"
    );
    let widest = widest_round_read_set(FALLBACK_N, &input, &result);
    assert_eq!(
        widest, FALLBACK_N,
        "edge (0, 1) must read the whole graph, got {widest}"
    );
    assert!(
        widest > MARK_LIMIT,
        "the read set must exceed the marking limit"
    );
    assert_thread_invariant("fallback", &dense, threshold);
    assert_fixture_barcode("fallback", &dense, threshold, 1);
}

#[test]
fn k64_64_zero_yield_v2() {
    // A triangle-free graph has no candidate anywhere, so the first round
    // finds nothing and the schedule stops. One round, an empty certificate,
    // and the input returned untouched.
    let dense = bipartite_dense();
    let result = v2_dense("k64_64", &dense, None, 4);
    assert_eq!(
        result.certificate.algorithm_version(),
        2,
        "the parallel entry point must tag version 2"
    );
    assert!(
        result.certificate.steps().is_empty(),
        "a triangle-free graph has no removable edge"
    );
    assert_eq!(result.stats.epochs, 1, "zero yield must take one round");
    assert_eq!(result.certificate.input_edge_count(), 4096, "input edges");
    assert_eq!(result.certificate.output_edge_count(), 4096, "output edges");
    assert_eq!(result.matrix.num_edges(), 4096, "surviving edges");
    assert_eq!(result.stats.witness_segments, 0, "no witness segments");
    assert_eq!(
        result.stats.max_common_neighborhood, 0,
        "no edge has a common neighbor"
    );
    assert_eq!(
        result.certificate.terminal_level(),
        1.0,
        "terminal level is the only edge value"
    );
    assert_reference_match("k64_64", &result, &reference_dense(&dense, None));

    let plain = dense_bars(&dense, 1, None, 2, 1, ALL_ON, false);
    let collapsed = dense_bars(&dense, 1, None, 2, 4, ALL_ON, true);
    assert_eq!(plain, collapsed, "K64,64: collapse changed the diagram");
    assert_eq!(essential_count(&collapsed, 0), 1, "K64,64 is connected");
    assert_eq!(essential_count(&collapsed, 1), 3969, "K64,64 cycle rank");
}

#[test]
fn k64_64_plus_k4_mixed_yield_v2() {
    // Only the K4 can yield: every bipartite edge is triangle-free and must
    // survive, so every removal step must sit inside the K4, and the round
    // structure must be the K4's own.
    let dense = bipartite_k4_dense();
    let result = v2_dense("k64_64+k4", &dense, None, 4);
    assert_eq!(
        result.certificate.input_edge_count(),
        4096 + 6,
        "input edges"
    );
    assert!(
        !result.certificate.steps().is_empty(),
        "the K4 must yield removals"
    );
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert!(
            u >= BIP_N && v >= BIP_N,
            "removal of ({u}, {v}) escaped the K4"
        );
    }
    let surviving_bipartite = result
        .matrix
        .edges()
        .filter(|&(u, v, _)| u < BIP_N && v < BIP_N)
        .count();
    assert_eq!(
        surviving_bipartite, 4096,
        "every bipartite edge must survive"
    );
    assert_eq!(
        round_widths(&result),
        vec![1, 2, 0],
        "the K4 keeps its own round structure inside the larger graph"
    );

    let plain = dense_bars(&dense, 1, None, 2, 1, ALL_ON, false);
    let collapsed = dense_bars(&dense, 1, None, 2, 4, ALL_ON, true);
    assert_eq!(plain, collapsed, "K64,64+K4: collapse changed the diagram");
    assert_eq!(essential_count(&collapsed, 0), 2, "two components");
    assert_eq!(essential_count(&collapsed, 1), 3969, "bipartite cycle rank");
}
