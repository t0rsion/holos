pub(crate) use holos_tda::collapse::verify::{verify_dense, verify_sparse};
pub(crate) use holos_tda::collapse::{
    CollapsedRips, RemovalStep, collapse_dense, collapse_dense_rounds_parallel, collapse_sparse,
    collapse_sparse_rounds_parallel,
};
pub(crate) use holos_tda::oracle::rips_persistence_oracle_mod;
pub(crate) use holos_tda::{
    Bar, CollapseSchedule, Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix,
    rips_persistence, rips_persistence_sparse,
};

pub(crate) const MODULI: [u32; 3] = [2, 3, 5];
pub(crate) const THREAD_COUNTS: [usize; 4] = [1, 2, 4, 8];
pub(crate) const ALL_ON: (bool, bool, bool) = (true, true, true);
pub(crate) const ALL_OFF: (bool, bool, bool) = (false, false, false);
/// Every clearing x emergent-pairs x apparent-pairs combination.
pub(crate) const TOGGLE_CROSS: [(bool, bool, bool); 8] = [
    (false, false, false),
    (false, false, true),
    (false, true, false),
    (false, true, true),
    (true, false, false),
    (true, false, true),
    (true, true, false),
    (true, true, true),
];

pub(crate) const BIP_N: usize = 128;
pub(crate) const K4_N: usize = 4;

pub(crate) struct Rng(pub(crate) u64);

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
// Battery inputs. Each one is small enough for the oracle at max_dim 2.

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
        p = p.with_edge_collapse();
    }
    p
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

pub(crate) fn essential_count(bars: &[Bar], dim: usize) -> usize {
    bars.iter()
        .filter(|b| b.dim == dim && b.death.is_infinite())
        .count()
}

pub(crate) fn finite_count(bars: &[Bar], dim: usize) -> usize {
    bars.iter()
        .filter(|b| b.dim == dim && b.death.is_finite())
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

pub(crate) fn sparse_from_edges(n: usize, edges: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(n, edges).unwrap()
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

pub(crate) fn thresholded_sparse(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Vec<(usize, usize, f64)> {
    let t = threshold.unwrap_or(f64::INFINITY);
    dist.edges().filter(|&(_, _, d)| d <= t).collect()
}

pub(crate) fn removed_edges(result: &CollapsedRips) -> Vec<(usize, usize, f64)> {
    let mut v: Vec<(usize, usize, f64)> = result
        .certificate
        .steps()
        .iter()
        .map(|s| {
            let (u, v) = s.edge();
            (u, v, s.value())
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    v
}

pub(crate) fn step_for(result: &CollapsedRips, edge: (usize, usize)) -> Option<&RemovalStep> {
    result.certificate.steps().iter().find(|s| s.edge() == edge)
}
