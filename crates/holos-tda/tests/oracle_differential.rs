//! Solver vs. independent oracle: exact multiset equality of diagrams.
//! Both sides read the same f64 matrix entries and take maxima only, so
//! births and deaths must agree bit-for-bit.

use holos_tda::oracle::{rips_persistence_oracle, rips_persistence_oracle_mod};
use holos_tda::{rips_persistence, Diagram, DistanceMatrix, RipsParams};

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

fn canonical(d: &Diagram) -> Vec<(usize, f64, f64)> {
    let mut v: Vec<_> = d.bars.iter().map(|b| (b.dim, b.birth, b.death)).collect();
    v.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
    });
    v
}

fn assert_same_diagram(a: &Diagram, b: &Diagram) {
    assert_eq!(canonical(a), canonical(b));
}

fn check(dist: &DistanceMatrix, max_dim: usize, threshold: Option<f64>) {
    for p in [2, 3, 5] {
        let expected = rips_persistence_oracle_mod(dist, max_dim, threshold, p);
        let mut params = RipsParams::new(max_dim).with_modulus(p);
        params.threshold = threshold;
        let got = rips_persistence(dist, &params).unwrap();
        assert_eq!(canonical(&got), canonical(&expected), "modulus {p}");
    }
}

#[test]
fn exhaustive_two_valued_matrices() {
    for n in 2..=5usize {
        let m = n * (n - 1) / 2;
        for mask in 0u32..1 << m {
            let data: Vec<f64> = (0..m)
                .map(|k| if mask >> k & 1 == 1 { 2.0 } else { 1.0 })
                .collect();
            let dist = DistanceMatrix::from_condensed(data).unwrap();
            for max_dim in 0..=2.min(n - 2) {
                check(&dist, max_dim, None);
            }
        }
    }
}

// Release gate: all 2^15 two-valued matrices on 6 points, dims 0..=3, all 8
// optimization-toggle combinations. The oracle runs once per matrix at
// max_dim 3. For a lower max_dim, the expectation is that result restricted
// to bars of dimension <= max_dim. Reduction decomposes by dimension, and
// the (k+1)-skeleton settles the essential dim-k classes.
#[test]
#[ignore = "exhaustive 6-point sweep; run with --ignored, preferably in release"]
fn exhaustive_six_point_sweep_all_dims_and_toggles() {
    exhaustive_six_point_sweep(2);
}

#[test]
#[ignore = "exhaustive 6-point sweep; run with --ignored, preferably in release"]
fn exhaustive_six_point_sweep_mod_3() {
    exhaustive_six_point_sweep(3);
}

// p = 3 only exercises coefficients +-1. p = 5 is the smallest field where
// stored pivots and inverses take values other than 1 and p-1.
#[test]
#[ignore = "exhaustive 6-point sweep; run with --ignored, preferably in release"]
fn exhaustive_six_point_sweep_mod_5() {
    exhaustive_six_point_sweep(5);
}

fn exhaustive_six_point_sweep(modulus: u32) {
    let threads = std::thread::available_parallelism().map_or(1, |p| p.get());
    let total = 1u32 << 15;
    let chunk = total.div_ceil(threads as u32);
    std::thread::scope(|scope| {
        for t in 0..threads as u32 {
            let lo = t * chunk;
            let hi = total.min(lo + chunk);
            scope.spawn(move || {
                for mask in lo..hi {
                    sweep_one_matrix(mask, modulus);
                }
            });
        }
    });
}

fn sweep_one_matrix(mask: u32, modulus: u32) {
    let data: Vec<f64> = (0..15)
        .map(|k| if mask >> k & 1 == 1 { 2.0 } else { 1.0 })
        .collect();
    let dist = DistanceMatrix::from_condensed(data).unwrap();
    let full = canonical(&rips_persistence_oracle_mod(&dist, 3, None, modulus));
    for max_dim in 0..=3usize {
        let expected: Vec<_> = full
            .iter()
            .copied()
            .filter(|&(dim, _, _)| dim <= max_dim)
            .collect();
        for bits in 0..8u8 {
            let mut params = RipsParams::default();
            params.max_dim = max_dim;
            params.threshold = None;
            params.modulus = modulus;
            params.use_emergent_pairs = bits & 1 != 0;
            params.use_apparent_pairs = bits & 2 != 0;
            params.use_clearing = bits & 4 != 0;
            let got = rips_persistence(&dist, &params).unwrap();
            assert_eq!(
                canonical(&got),
                expected,
                "mask {mask:#06x}, modulus {modulus}, max_dim {max_dim}, toggles {bits:03b}"
            );
        }
    }
}

// Regression: emergent/apparent shortcuts once reconstructed the wrong vertex
// set for a candidate cofacet. That invented spurious H1/H2 bars on this
// matrix.
#[test]
fn regression_emergent_apparent_wrong_vertex_set() {
    let data = vec![
        1.0,
        2.0,
        2.0,
        1.0,
        1.0,
        1.0,
        1.0,
        1.0,
        1.0,
        f64::INFINITY,
        2.0,
        1.0,
        1.0,
        1.0,
        1.0,
    ];
    let dist = DistanceMatrix::from_condensed(data).unwrap();
    let expected = rips_persistence_oracle(&dist, 2, None);
    for bits in 0..8u8 {
        let mut params = RipsParams::default();
        params.max_dim = 2;
        params.threshold = None;
        params.modulus = 2;
        params.use_emergent_pairs = bits & 1 != 0;
        params.use_apparent_pairs = bits & 2 != 0;
        params.use_clearing = bits & 4 != 0;
        let got = rips_persistence(&dist, &params).unwrap();
        assert_eq!(got.in_dim(1).count(), 0, "toggles {bits:03b}: H1 not empty");
        assert_eq!(got.in_dim(2).count(), 0, "toggles {bits:03b}: H2 not empty");
        assert_same_diagram(&got, &expected);
    }
}

// Regression: the earlier repro for the same bug family. H2 must be empty.
#[test]
fn regression_spurious_h2_bar() {
    let data = vec![
        0.5, 2.0, 2.0, 0.5, 1.0, 2.0, 2.0, 2.0, 0.5, 0.5, 0.5, 0.5, 1.0, 2.5, 1.5,
    ];
    let dist = DistanceMatrix::from_condensed(data).unwrap();
    let expected = rips_persistence_oracle(&dist, 2, None);
    for bits in 0..8u8 {
        let mut params = RipsParams::default();
        params.max_dim = 2;
        params.threshold = None;
        params.modulus = 2;
        params.use_emergent_pairs = bits & 1 != 0;
        params.use_apparent_pairs = bits & 2 != 0;
        params.use_clearing = bits & 4 != 0;
        let got = rips_persistence(&dist, &params).unwrap();
        assert_eq!(got.in_dim(2).count(), 0, "toggles {bits:03b}: H2 not empty");
        assert_same_diagram(&got, &expected);
    }
}

// Regression: with clearing disabled the solver once suppressed essential
// classes whose columns it had already reduced to zero.
#[test]
fn regression_clearing_disabled_no_false_essential_class() {
    let dist = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap();
    let mut params = RipsParams::default();
    params.max_dim = 2;
    params.threshold = None;
    params.modulus = 2;
    params.use_emergent_pairs = false;
    params.use_apparent_pairs = false;
    params.use_clearing = false;
    let got = rips_persistence(&dist, &params).unwrap();
    assert_eq!(
        canonical(&got),
        vec![(0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, f64::INFINITY)]
    );
}

#[test]
fn randomized_matrices_with_ties_and_infinities() {
    let palette = [0.5, 1.0, 1.5, 2.0, 2.5, f64::INFINITY];
    let mut rng = Rng::new(0x9e3779b97f4a7c15);
    for _ in 0..200 {
        let n = 2 + rng.below(8);
        let m = n * (n - 1) / 2;
        let data: Vec<f64> = (0..m)
            .map(|_| {
                if rng.below(12) == 0 {
                    0.0
                } else {
                    palette[rng.below(palette.len())]
                }
            })
            .collect();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let max_dim = rng.below(3).min(n - 2);
        let threshold = if rng.below(2) == 0 {
            None
        } else {
            Some(3.0 * rng.uniform())
        };
        check(&dist, max_dim, threshold);
    }
}

#[test]
fn randomized_euclidean_point_clouds() {
    let mut rng = Rng::new(0x51ce_5eed_0dd5_ea11);
    for _ in 0..50 {
        let n = 3 + rng.below(10);
        let ambient = 2 + rng.below(2);
        let points: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..ambient).map(|_| 2.0 * rng.uniform() - 1.0).collect())
            .collect();
        let dist = DistanceMatrix::from_points(&points).unwrap();
        let max_dim = rng.below(3).min(n - 2);
        check(&dist, max_dim, None);
    }
}

#[test]
fn optimization_toggles_never_change_the_diagram() {
    let mut rng = Rng::new(0x0ff1_ce0f_f1ce_0ff1);
    for _ in 0..30 {
        let n = 4 + rng.below(7);
        let points: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..3).map(|_| 2.0 * rng.uniform() - 1.0).collect())
            .collect();
        let dist = DistanceMatrix::from_points(&points).unwrap();
        let max_dim = rng.below(3).min(n - 2);
        let threshold = if rng.below(2) == 0 {
            None
        } else {
            Some(1.0 + 2.0 * rng.uniform())
        };
        for p in [2, 3] {
            let expected = rips_persistence_oracle_mod(&dist, max_dim, threshold, p);
            for bits in 0..8u8 {
                let mut params = RipsParams::default();
                params.max_dim = max_dim;
                params.threshold = threshold;
                params.modulus = p;
                params.use_emergent_pairs = bits & 1 != 0;
                params.use_apparent_pairs = bits & 2 != 0;
                params.use_clearing = bits & 4 != 0;
                let got = rips_persistence(&dist, &params).unwrap();
                assert_eq!(
                    canonical(&got),
                    canonical(&expected),
                    "modulus {p}, toggles {bits:03b}"
                );
            }
        }
    }
}
