//! Aggressive stress gates, all `#[ignore]`: run with
//! `cargo test --release --test stress -- --ignored` (add
//! `STRESS_ITERS=...` to scale the fuzz budgets). Deterministic seeds.

use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{rips_persistence, Bar, Diagram, DistanceMatrix, RipsParams};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn canonical(d: &Diagram) -> Vec<(usize, f64, f64)> {
    let mut v: Vec<_> = d.bars.iter().map(|b| (b.dim, b.birth, b.death)).collect();
    v.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
    });
    v
}

fn oracle_bars(dist: &DistanceMatrix, max_dim: usize, threshold: Option<f64>, p: u32) -> Vec<Bar> {
    rips_persistence_oracle_mod(dist, max_dim, threshold, p).bars
}

fn check(dist: &DistanceMatrix, max_dim: usize, threshold: Option<f64>, p: u32, ctx: &str) {
    let mut params = RipsParams::new(max_dim).with_modulus(p);
    params.threshold = threshold;
    let solver = rips_persistence(dist, &params).unwrap();
    let oracle = Diagram {
        bars: oracle_bars(dist, max_dim, threshold, p),
    };
    assert_eq!(
        canonical(&solver),
        canonical(&oracle),
        "solver != oracle: {ctx}"
    );
}

fn iters(default: usize) -> usize {
    std::env::var("STRESS_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Random matrices over mixed regimes: continuous, tie-heavy quantized,
/// {0.5,1,2,inf} discrete, zero-distance duplicates. Every prime class,
/// random toggles, random dims and thresholds.
#[test]
#[ignore = "stress; run with --ignored in release"]
fn fuzz_random_matrices_all_fields() {
    let primes = [2u32, 3, 5, 7, 13, 251, 32749];
    let mut rng = Rng::new(0x5eed_0f02);
    for it in 0..iters(20_000) {
        let n = 4 + rng.below(6); // 4..=9
        let regime = rng.below(4);
        let m = n * (n - 1) / 2;
        let mut data = Vec::with_capacity(m);
        for _ in 0..m {
            let d = match regime {
                0 => rng.uniform(),
                1 => (rng.uniform() * 4.0).ceil() / 4.0,
                2 => [0.5, 1.0, 2.0, f64::INFINITY][rng.below(4)],
                _ => {
                    if rng.below(4) == 0 {
                        0.0
                    } else {
                        rng.uniform()
                    }
                }
            };
            data.push(d);
        }
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let max_dim = 1 + rng.below(n - 1);
        let threshold = match rng.below(4) {
            0 => None,
            1 => Some(0.0),
            2 => Some(rng.uniform() * 2.0),
            _ => Some(f64::INFINITY),
        };
        let p = primes[rng.below(primes.len())];
        let mut params = RipsParams::new(max_dim).with_modulus(p);
        params.threshold = threshold;
        params.use_emergent_pairs = rng.below(2) == 0;
        params.use_apparent_pairs = rng.below(2) == 0;
        params.use_clearing = rng.below(2) == 0;
        let solver = rips_persistence(&dist, &params).unwrap();
        let oracle = Diagram {
            bars: oracle_bars(&dist, max_dim, threshold, p),
        };
        assert_eq!(
            canonical(&solver),
            canonical(&oracle),
            "iter {it}: n={n} regime={regime} max_dim={max_dim} p={p} \
             threshold={threshold:?} toggles=({},{},{})",
            params.use_emergent_pairs,
            params.use_apparent_pairs,
            params.use_clearing
        );
    }
}

/// All 2^21 seven-point 0/1-weight matrices vs the oracle, dims 0..=2,
/// at p = 2 and p = 3. The seven-point analogue of the six-point release
/// gate; roughly two million solver/oracle comparisons per field.
#[test]
#[ignore = "stress; several minutes in release"]
fn exhaustive_seven_point_zero_one_sweep() {
    let n = 7;
    let m = n * (n - 1) / 2; // 21 bits
    for p in [2u32, 3] {
        for mask in 0u32..(1 << m) {
            let data: Vec<f64> = (0..m)
                .map(|i| if mask >> i & 1 == 1 { 1.0 } else { 0.5 })
                .collect();
            let dist = DistanceMatrix::from_condensed(data).unwrap();
            check(&dist, 2, None, p, &format!("mask {mask:#x} p {p}"));
        }
    }
}

/// Everything pairwise-equidistant is a cone at that scale: n-1 finite H0
/// bars, one essential, nothing above. Checked directly (no oracle) up to
/// n = 40 and dim 3, plus below-threshold truncation.
#[test]
#[ignore = "stress; run with --ignored in release"]
fn all_equal_distances_are_cones() {
    for n in 2..=40 {
        let d = 1.5;
        let data = vec![d; n * (n - 1) / 2];
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let diagram = rips_persistence(&dist, &RipsParams::new(3)).unwrap();
        let h0: Vec<_> = diagram.in_dim(0).collect();
        assert_eq!(h0.len(), n);
        assert_eq!(h0.iter().filter(|b| b.is_essential()).count(), 1);
        assert!(h0.iter().all(|b| b.is_essential() || b.death == d));
        assert_eq!(diagram.bars.len(), n, "no bars above dim 0 for n={n}");

        let cut = rips_persistence(&dist, &RipsParams::new(3).with_threshold(d / 2.0)).unwrap();
        assert_eq!(cut.bars.len(), n);
        assert!(cut.bars.iter().all(|b| b.dim == 0 && b.is_essential()));
    }
}

/// Ultrametrics: at every scale the neighborhood graph is a disjoint union
/// of cliques, so the flag complex is a disjoint union of simplices and
/// H_{>=1} is empty. Structural check to n = 60, oracle check to n = 9.
#[test]
#[ignore = "stress; run with --ignored in release"]
fn ultrametrics_have_no_higher_homology() {
    let mut rng = Rng::new(0x0a17_a3e7);
    for it in 0..iters(20_000).min(2_000) {
        let n = 4 + rng.below(57); // 4..=60
                                   // Random binary merge tree: assign each point a leaf path; distance
                                   // is the height of the lowest common ancestor.
        let depth = 6;
        let labels: Vec<u32> = (0..n)
            .map(|_| rng.next() as u32 & ((1 << depth) - 1))
            .collect();
        let mut data = Vec::with_capacity(n * (n - 1) / 2);
        for i in 1..n {
            for j in 0..i {
                let diff = labels[i] ^ labels[j];
                let lca = 32 - diff.leading_zeros(); // 0 when identical
                data.push(lca as f64);
            }
        }
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let diagram = rips_persistence(&dist, &RipsParams::new(2)).unwrap();
        assert!(
            diagram.bars.iter().all(|b| b.dim == 0),
            "iter {it}: ultrametric produced homology above dim 0 (n={n})"
        );
        if n <= 9 {
            check(&dist, 2, None, 3, &format!("ultrametric iter {it} n={n}"));
        }
    }
}

/// The largest accepted prime exercises the full inverse table and keeps
/// residues far from +-1. Exhaustive over five-point 0/1 matrices plus the
/// projective plane (whose torsion must vanish at any odd prime).
#[test]
#[ignore = "stress; run with --ignored in release"]
fn maximum_prime_modulus() {
    let p = 32749u32;
    let n = 5;
    let m = n * (n - 1) / 2;
    for mask in 0u32..(1 << m) {
        let data: Vec<f64> = (0..m)
            .map(|i| if mask >> i & 1 == 1 { 1.0 } else { 0.5 })
            .collect();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        check(&dist, 2, None, p, &format!("max-prime mask {mask:#x}"));
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/projective_plane.lower_distance_matrix");
    let dist = holos_tda::io::read_lower_distance_matrix(&path).unwrap();
    let d = rips_persistence(&dist, &RipsParams::new(2).with_modulus(p)).unwrap();
    assert_eq!(d.in_dim(1).count(), 0);
    assert_eq!(d.in_dim(2).count(), 0);
}
