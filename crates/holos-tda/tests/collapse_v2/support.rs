use holos_tda::collapse::verify::{verify_dense, verify_sparse};
use holos_tda::collapse::{
    CollapsedRips, RemovalStep, collapse_dense_rounds_parallel, collapse_sparse_rounds_parallel,
};
use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{
    Bar, CollapseSchedule, Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix,
    rips_persistence, rips_persistence_sparse,
};

pub(crate) const MODULI: [u32; 3] = [2, 3, 5];
pub(crate) const COLLAPSE_THREADS: [usize; 4] = [1, 2, 4, 8];
pub(crate) const REDUCER_THREADS: [usize; 2] = [1, 4];
pub(crate) const ALL_ON: (bool, bool, bool) = (true, true, true);
pub(crate) const ALL_OFF: (bool, bool, bool) = (false, false, false);
/// The production marking limit. Above this many vertices in the removed
/// edge's closed common neighborhood, the collapser drops fine marking and
/// retests every live edge in the next round.
pub(crate) const MARK_LIMIT: usize = 64;

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

pub(crate) fn params(
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

pub(crate) type StepBits = (usize, usize, u64, usize, Vec<(u64, usize)>);

/// Certificate steps with every float taken by bits, so `-0.0` and `0.0`
/// do not compare equal.
pub(crate) fn step_bits(cert: &holos_tda::collapse::CollapseCertificate) -> Vec<StepBits> {
    cert.steps()
        .iter()
        .map(|s| {
            let (u, v) = s.edge();
            (
                u,
                v,
                s.value().to_bits(),
                s.position().number(),
                s.witnesses()
                    .iter()
                    .map(|&(a, w)| (a.to_bits(), w))
                    .collect(),
            )
        })
        .collect()
}

pub(crate) fn canon(diagram: &Diagram) -> Vec<Bar> {
    let mut d = diagram.clone();
    d.canonicalize();
    d.bars
}

pub(crate) fn dense_bars(
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

pub(crate) fn sparse_bars(
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

pub(crate) fn oracle_bars(
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
pub(crate) fn collapsed_bars(
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

pub(crate) fn essential_count(bars: &[Bar], dim: usize) -> usize {
    bars.iter()
        .filter(|b| b.dim == dim && b.death.is_infinite())
        .count()
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

pub(crate) fn edge_list(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, f64)> {
    matrix.edges().collect()
}

/// The edges the collapse must describe: finite, at or below the resolved
/// threshold, in ascending endpoint order.
pub(crate) fn thresholded_dense(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
) -> Vec<(usize, usize, f64)> {
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

pub(crate) fn step_for(result: &CollapsedRips, edge: (usize, usize)) -> Option<&RemovalStep> {
    result.certificate.steps().iter().find(|s| s.edge() == edge)
}

pub(crate) fn removed_set(result: &CollapsedRips) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = result
        .certificate
        .steps()
        .iter()
        .map(|s| s.edge())
        .collect();
    v.sort_unstable();
    v
}

/// Steps per round, one entry per schedule position. The last entry is the
/// closing round, which removes nothing.
pub(crate) fn round_widths(result: &CollapsedRips) -> Vec<usize> {
    let mut widths = vec![0usize; result.stats.epochs];
    for step in result.certificate.steps() {
        let round = step.position().number();
        assert!(
            round >= 1 && round <= widths.len(),
            "step round {round} outside the {} recorded epochs",
            widths.len()
        );
        widths[round - 1] += 1;
    }
    widths
}

pub(crate) fn adjacency(n: usize, edges: &[(usize, usize, f64)]) -> Vec<Vec<bool>> {
    let mut adj = vec![vec![false; n]; n];
    for &(u, v, _) in edges {
        adj[u][v] = true;
        adj[v][u] = true;
    }
    adj
}

/// The closed common neighborhood S(e) = N[u] intersect N[v], sorted.
pub(crate) fn common_closed_set(adj: &[Vec<bool>], u: usize, v: usize) -> Vec<usize> {
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
/// production counter: rounds group by schedule position, and every step of a
/// round reads the graph as it stood before the round.
pub(crate) fn widest_round_read_set(
    n: usize,
    input: &[(usize, usize, f64)],
    result: &CollapsedRips,
) -> usize {
    let mut adj = adjacency(n, input);
    let steps = result.certificate.steps();
    let mut widest = 0;
    let mut i = 0;
    while i < steps.len() {
        let round = steps[i].position().number();
        let mut j = i;
        while j < steps.len() && steps[j].position().number() == round {
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
pub(crate) fn v2_dense(
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

pub(crate) fn v2_sparse(
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
