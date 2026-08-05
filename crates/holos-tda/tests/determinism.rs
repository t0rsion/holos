//! Cross-thread determinism gate: the diagram must be byte-for-byte identical
//! at every thread count. The parallel reducer reduces each dimension
//! concurrently and must reproduce the serial diagram exactly. These tests
//! therefore assert only on `Diagram.bars`, never on internal state.
//!
//! A cheap subset runs under `cargo test`. The exhaustive sweep is `#[ignore]`
//! and scales with `STRESS_ITERS` (`cargo test --release --test determinism --
//! --ignored`).

use std::f64::consts::PI;

use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{
    rips_persistence, rips_persistence_sparse, Diagram, DistanceMatrix, RipsParams,
    SparseDistanceMatrix,
};

const THREAD_COUNTS: [usize; 4] = [1, 2, 4, 8];
const MODULI: [u32; 4] = [2, 3, 5, 7];

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
    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn iters(default: usize) -> usize {
    std::env::var("STRESS_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// One determinism scenario: a matrix plus its filtration parameters. Some
/// cases only make sense at a specific threshold.
struct Case {
    name: String,
    dist: DistanceMatrix,
    threshold: Option<f64>,
    max_dim: usize,
}

fn from_points(points: &[Vec<f64>]) -> DistanceMatrix {
    DistanceMatrix::from_points(points).unwrap()
}

// Uniform cloud in the unit cube: distinct coordinates, generic diameters.
fn random_cloud(seed: u64, n: usize, coord_dim: usize, max_dim: usize) -> Case {
    let mut rng = Rng::new(seed);
    let points: Vec<Vec<f64>> = (0..n)
        .map(|_| (0..coord_dim).map(|_| rng.uniform()).collect())
        .collect();
    Case {
        name: format!("random_cloud(seed={seed},n={n},d={coord_dim})"),
        dist: from_points(&points),
        threshold: None,
        max_dim,
    }
}

// Integer lattice: pairwise distances collapse onto {1, sqrt2, 2, sqrt5, ...},
// so many simplices share a diameter. That tie regime is where a parallel
// reducer is most likely to reorder work.
fn grid(side: usize, max_dim: usize) -> Case {
    let points: Vec<Vec<f64>> = (0..side)
        .flat_map(|x| (0..side).map(move |y| vec![x as f64, y as f64]))
        .collect();
    Case {
        name: format!("grid({side}x{side})"),
        dist: from_points(&points),
        threshold: None,
        max_dim,
    }
}

// Regular n-gon on the unit circle: a persistent H1 class. `threshold` bounds
// the filtration so large samples stay affordable at dim 2.
fn circle(n: usize, max_dim: usize, threshold: Option<f64>) -> Case {
    let points: Vec<Vec<f64>> = (0..n)
        .map(|k| {
            let t = 2.0 * PI * k as f64 / n as f64;
            vec![t.cos(), t.sin()]
        })
        .collect();
    Case {
        name: format!("circle({n})"),
        dist: from_points(&points),
        threshold,
        max_dim,
    }
}

// Two tight clusters 100 apart. A threshold below the gap keeps them
// disconnected, so two H0 components survive to infinity.
fn disconnected(seed: u64, per: usize, max_dim: usize) -> Case {
    let mut rng = Rng::new(seed);
    let mut points = Vec::new();
    for cluster in 0..2 {
        let offset = cluster as f64 * 100.0;
        for _ in 0..per {
            points.push(vec![offset + rng.uniform(), rng.uniform()]);
        }
    }
    Case {
        name: format!("disconnected(seed={seed},per={per})"),
        dist: from_points(&points),
        threshold: Some(3.0),
        max_dim,
    }
}

// Coincident points inject zero-diameter simplices (distance exactly 0).
fn duplicates(seed: u64, distinct: usize, copies: usize, max_dim: usize) -> Case {
    let mut rng = Rng::new(seed);
    let mut points = Vec::new();
    for _ in 0..distinct {
        let p = vec![rng.uniform(), rng.uniform()];
        for _ in 0..copies {
            points.push(p.clone());
        }
    }
    Case {
        name: format!("duplicates(seed={seed},distinct={distinct},copies={copies})"),
        dist: from_points(&points),
        threshold: None,
        max_dim,
    }
}

// Condensed matrix with +inf entries: absent edges never enter the
// filtration. This exercises the disconnected/infinite path directly.
fn infinite_edges(seed: u64, n: usize, max_dim: usize) -> Case {
    let mut rng = Rng::new(seed);
    let palette = [0.5, 1.0, 1.5, 2.0];
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for _ in 0..n * (n - 1) / 2 {
        if rng.uniform() < 0.25 {
            condensed.push(f64::INFINITY);
        } else {
            condensed.push(palette[(rng.next_u64() % palette.len() as u64) as usize]);
        }
    }
    Case {
        name: format!("infinite_edges(seed={seed},n={n})"),
        dist: DistanceMatrix::from_condensed(condensed).unwrap(),
        threshold: Some(2.0),
        max_dim,
    }
}

fn compute(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
) -> Diagram {
    let mut params = RipsParams::new(max_dim)
        .with_modulus(modulus)
        .with_threads(threads);
    params.threshold = threshold;
    params.use_clearing = toggles.0;
    params.use_emergent_pairs = toggles.1;
    params.use_apparent_pairs = toggles.2;
    let mut diagram = rips_persistence(dist, &params).unwrap();
    diagram.canonicalize();
    diagram
}

// The gate itself: canonicalized bars at threads {2,4,8} must equal the
// threads=1 bars exactly, for one field and one toggle configuration.
fn assert_thread_invariant(case: &Case, modulus: u32, toggles: (bool, bool, bool)) {
    let base = compute(
        &case.dist,
        case.max_dim,
        case.threshold,
        modulus,
        1,
        toggles,
    );
    for &threads in &THREAD_COUNTS[1..] {
        let got = compute(
            &case.dist,
            case.max_dim,
            case.threshold,
            modulus,
            threads,
            toggles,
        );
        assert_eq!(
            base.bars, got.bars,
            "{}: threads={threads} diverged from threads=1 (modulus={modulus}, \
             toggles={toggles:?})",
            case.name
        );
    }
}

// Full cross of moduli x the 2^3 optimization matrix for one case.
fn sweep_case(case: &Case) {
    for &modulus in &MODULI {
        for bits in 0u8..8 {
            let toggles = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
            assert_thread_invariant(case, modulus, toggles);
        }
    }
}

/// Independent oracle cross-check: with all optimizations on over Z/2, the
/// thread-invariant diagram also equals the brute-force reference. Small
/// inputs only (the oracle enumerates every simplex).
fn assert_matches_oracle(case: &Case) {
    let solver = compute(
        &case.dist,
        case.max_dim,
        case.threshold,
        2,
        1,
        (true, true, true),
    );
    let mut oracle = rips_persistence_oracle_mod(&case.dist, case.max_dim, case.threshold, 2);
    oracle.canonicalize();
    assert_eq!(
        solver.bars, oracle.bars,
        "{}: solver disagrees with oracle",
        case.name
    );
}

// Small, structurally varied inputs covering every family. Cheap enough for
// the default `cargo test` and for the oracle cross-check.
fn default_cases() -> Vec<Case> {
    vec![
        random_cloud(0x1111, 8, 2, 2),
        random_cloud(0x2222, 11, 3, 2),
        grid(3, 2),
        circle(9, 2, None),
        disconnected(0x3333, 4, 1),
        duplicates(0x4444, 4, 2, 2),
        infinite_edges(0x5555, 8, 2),
    ]
}

/// Default gate: every input family, the full modulus x toggle x thread cross,
/// plus an oracle cross-check.
#[test]
fn diagram_is_thread_invariant() {
    for case in default_cases() {
        sweep_case(&case);
        assert_matches_oracle(&case);
    }
}

/// Exhaustive sweep: larger clouds and many seeds per family, the full
/// modulus x toggle x thread cross on each. Scales with STRESS_ITERS.
#[test]
#[ignore = "stress; run with --ignored in release"]
fn diagram_is_thread_invariant_exhaustive() {
    let seeds = iters(24);
    for s in 0..seeds as u64 {
        let seed = 0xdead_0000 ^ s.wrapping_mul(0x9e37_79b9);
        sweep_case(&random_cloud(seed, 20 + (s as usize % 41), 3, 1));
        sweep_case(&disconnected(seed, 12 + (s as usize % 18), 1));
        sweep_case(&duplicates(seed, 6 + (s as usize % 6), 3, 2));
        sweep_case(&infinite_edges(seed, 10 + (s as usize % 6), 2));
    }
    for side in 3..=5 {
        sweep_case(&grid(side, 2));
    }
    // Small circles run at dim 2 (full flag complex). Larger ones run at
    // dim 1 with a radius bound. That still carries an H1 class, without the
    // dim-2 blowup.
    for n in [12, 16, 20] {
        sweep_case(&circle(n, 2, None));
    }
    for n in [30, 45, 60] {
        sweep_case(&circle(n, 1, Some(0.9)));
    }
}

// K64,64 plus a disjoint K4, every present edge at distance 1. The bipartite
// component contributes 4,096 edges, which crosses the engine's
// parallel-assembly threshold (PARALLEL_ASSEMBLE_MIN = 4,096), and it has no
// triangles. The K4 supplies a small nonempty triangle set, so chunk
// concatenation and the column sort both run. The barcode is known: two
// essential H0 classes, b1(K64,64) = 4096 - 128 + 1 = 3,969 essential H1
// classes, no finite H1 bar, and no H2 bar (the K4's candidate H2 class is
// born and dies at the same diameter, so it is suppressed).
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

fn bipartite_k4_sparse() -> SparseDistanceMatrix {
    let n = BIP_N + K4_N;
    let mut triplets = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = bipartite_k4_dist(i, j);
            if d.is_finite() {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
}

fn assert_bipartite_k4_structure(diagram: &Diagram, label: &str) {
    let count = |dim: usize, essential: bool| {
        diagram
            .bars
            .iter()
            .filter(|b| b.dim == dim && b.death.is_infinite() == essential)
            .count()
    };
    assert_eq!(count(0, true), 2, "{label}: essential H0");
    assert_eq!(count(1, true), 3969, "{label}: essential H1");
    assert_eq!(count(1, false), 0, "{label}: finite H1");
    assert_eq!(count(2, true) + count(2, false), 0, "{label}: H2");
}

fn bipartite_k4_compute(
    sparse: Option<&SparseDistanceMatrix>,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
) -> Diagram {
    let mut params = RipsParams::new(2)
        .with_modulus(modulus)
        .with_threads(threads);
    params.threshold = threshold;
    params.use_clearing = toggles.0;
    params.use_emergent_pairs = toggles.1;
    params.use_apparent_pairs = toggles.2;
    let mut diagram = match sparse {
        Some(s) => rips_persistence_sparse(s, &params).unwrap(),
        None => rips_persistence(dense, &params).unwrap(),
    };
    diagram.canonicalize();
    diagram
}

// Gate for the parallel assembly branch: dense and sparse, at and above the
// real threshold, serial and parallel bars must match exactly, and the
// barcode must match the known structure. Clearing and apparent pairs are
// the two toggles that assembly consults, so one run disables both.
#[test]
fn parallel_assembly_above_threshold_is_thread_invariant() {
    let dense = bipartite_k4_dense();
    let sparse = bipartite_k4_sparse();
    let all_on = (true, true, true);
    for source in [None, Some(&sparse)] {
        let kind = if source.is_some() { "sparse" } else { "dense" };
        for &modulus in &[2u32, 5] {
            for threshold in [None, Some(2.0)] {
                let label = format!("{kind} p={modulus} threshold={threshold:?}");
                let base = bipartite_k4_compute(source, &dense, threshold, modulus, 1, all_on);
                assert_bipartite_k4_structure(&base, &label);
                for threads in [2usize, 8] {
                    let got =
                        bipartite_k4_compute(source, &dense, threshold, modulus, threads, all_on);
                    assert_eq!(base.bars, got.bars, "{label}: threads={threads}");
                }
            }
        }
    }
    // Clearing and apparent pairs off, p=2, dense, serial vs 8 threads.
    let stripped = (false, true, false);
    let base = bipartite_k4_compute(None, &dense, None, 2, 1, stripped);
    assert_bipartite_k4_structure(&base, "dense stripped");
    let got = bipartite_k4_compute(None, &dense, None, 2, 8, stripped);
    assert_eq!(base.bars, got.bars, "dense stripped: threads=8");
}
