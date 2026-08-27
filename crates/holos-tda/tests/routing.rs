//! Engine routing: the three settings of [`Engine`], crossed with the
//! three settings of [`DenseStorage`], must give one diagram, bit for bit,
//! on every input.
//!
//! The bars carry f64 births and deaths, so the comparison is on bits.
//! Equal bits is a stronger statement than equal numbers: it separates 0.0
//! from -0.0 and admits no rounding.

use holos_tda::{DenseStorage, Diagram, DistanceMatrix, Engine, RipsParams, rips_persistence};

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

fn bits(d: &Diagram) -> Vec<(usize, u64, u64)> {
    d.bars
        .iter()
        .map(|b| (b.dim, b.birth.to_bits(), b.death.to_bits()))
        .collect()
}

/// The diagram of one input under every engine and storage setting,
/// asserted equal by bits against the forced-dense compact one. The
/// storage form reaches only a dense run, so `Sparse` runs it once.
fn assert_engines_agree(dist: &DistanceMatrix, params: &RipsParams, case: &str) {
    let reference = rips_persistence(
        dist,
        &params
            .clone()
            .with_engine(Engine::Dense)
            .with_dense_storage(DenseStorage::Compact),
    )
    .unwrap();
    for engine in [Engine::Auto, Engine::Dense, Engine::Sparse] {
        for storage in [
            DenseStorage::Auto,
            DenseStorage::Compact,
            DenseStorage::Square,
        ] {
            let got = rips_persistence(
                dist,
                &params
                    .clone()
                    .with_engine(engine)
                    .with_dense_storage(storage),
            )
            .unwrap();
            assert_eq!(
                bits(&got),
                bits(&reference),
                "{case}, {engine:?}, {storage:?}"
            );
        }
    }
}

/// A square grid of `side * side` points, one unit apart.
fn grid(side: usize) -> DistanceMatrix {
    let mut points = Vec::new();
    for i in 0..side {
        for j in 0..side {
            points.push(vec![i as f64, j as f64]);
        }
    }
    DistanceMatrix::from_points(&points).unwrap()
}

#[test]
fn fixtures_agree_across_thresholds_moduli_and_dimensions() {
    let inf = f64::INFINITY;
    let s = 2.0f64.sqrt();
    let fixtures: [(&str, Vec<f64>); 5] = [
        ("triangle", vec![1.0, 1.0, 1.0]),
        ("square", vec![1.0, s, 1.0, 1.0, s, 1.0]),
        ("two components", vec![1.0, inf, inf, inf, inf, 1.0]),
        ("zero edge", vec![0.0, 1.0, 1.0, 2.0, 2.0, 2.0]),
        (
            "all equal",
            vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
    ];
    for (name, data) in fixtures {
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        for max_dim in 0..=2 {
            for modulus in [2u32, 3, 5] {
                for threshold in [None, Some(0.0), Some(1.0), Some(1.5), Some(1e9)] {
                    let mut params = RipsParams::new(max_dim).with_modulus(modulus);
                    params.threshold = threshold;
                    assert_engines_agree(
                        &dist,
                        &params,
                        &format!("{name}, max_dim {max_dim}, p {modulus}, T {threshold:?}"),
                    );
                }
            }
        }
    }
}

#[test]
fn randomized_matrices_with_ties_and_infinities_agree() {
    let palette = [0.5, 1.0, 1.5, 2.0, 2.5, f64::INFINITY];
    let mut rng = Rng::new(0x726f_7574_696e_6701);
    for case in 0..120 {
        let n = 2 + rng.below(9);
        let data: Vec<f64> = (0..n * (n - 1) / 2)
            .map(|_| {
                if rng.below(10) == 0 {
                    0.0
                } else {
                    palette[rng.below(palette.len())]
                }
            })
            .collect();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let mut params = RipsParams::new(rng.below(3).min(n - 2))
            .with_modulus([2u32, 3, 5][rng.below(3)])
            .with_threads([1usize, 2, 4][rng.below(3)]);
        params.threshold = match rng.below(3) {
            0 => None,
            1 => Some(3.0 * rng.uniform()),
            _ => Some(f64::INFINITY),
        };
        assert_engines_agree(&dist, &params, &format!("random matrix {case}"));
    }
}

#[test]
fn randomized_point_clouds_agree() {
    let mut rng = Rng::new(0x726f_7574_696e_6702);
    for case in 0..30 {
        let n = 4 + rng.below(12);
        let ambient = 2 + rng.below(3);
        let points: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..ambient).map(|_| 2.0 * rng.uniform() - 1.0).collect())
            .collect();
        let dist = DistanceMatrix::from_points(&points).unwrap();
        let mut params = RipsParams::new(rng.below(3).min(n - 2));
        params.threshold = if rng.below(2) == 0 {
            None
        } else {
            Some(0.5 + 2.0 * rng.uniform())
        };
        assert_engines_agree(&dist, &params, &format!("random cloud {case}"));
    }
}

// Four hundred points at a threshold that keeps four edges per point is a
// low density well above the frozen point cutoff, so Auto routes it. The
// unit test inside the library pins the rule against its constants.
#[test]
fn a_large_low_density_input_agrees_at_every_thread_count() {
    let dist = grid(20);
    for threads in [1usize, 2, 4] {
        let params = RipsParams::new(1).with_threads(threads).with_threshold(1.5);
        assert_engines_agree(&dist, &params, &format!("grid, {threads} threads"));
    }
}

/// A band matrix on `n` points: `d(i, j)` is `|i - j|` within `width`, and
/// absent outside it. Every row holds an absent pair, so the enclosing
/// radius is infinite.
fn band(n: usize, width: usize) -> DistanceMatrix {
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            data.push(if i - j <= width {
                (i - j) as f64
            } else {
                f64::INFINITY
            });
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

/// Two grids of `side * side` points, `gap` apart. Every distance is
/// finite, and a threshold under `gap` leaves two components.
fn two_clusters(side: usize, gap: f64) -> DistanceMatrix {
    let mut points = Vec::new();
    for cluster in 0..2 {
        for i in 0..side {
            for j in 0..side {
                points.push(vec![i as f64 + cluster as f64 * gap, j as f64]);
            }
        }
    }
    DistanceMatrix::from_points(&points).unwrap()
}

// An infinite threshold admits every finite pair and no absent one, so a
// matrix of mostly absent pairs is sparse at that threshold and Auto routes
// it. A disconnected input reaches the same threshold through its default,
// because its enclosing radius is infinite.
#[test]
fn absent_edges_agree_at_an_infinite_threshold() {
    let dist = band(40, 3);
    assert_eq!(dist.enclosing_radius(), f64::INFINITY);
    for max_dim in 0..=2 {
        for modulus in [2u32, 3, 5] {
            for threshold in [None, Some(f64::INFINITY), Some(1e300), Some(2.0)] {
                let mut params = RipsParams::new(max_dim).with_modulus(modulus);
                params.threshold = threshold;
                assert_engines_agree(
                    &dist,
                    &params,
                    &format!("band, max_dim {max_dim}, p {modulus}, T {threshold:?}"),
                );
            }
        }
    }
}

// Finite distances throughout, but a threshold that separates the two
// clusters. The conversion must keep the isolated vertices and their
// essential H0 bars, at every thread count.
#[test]
fn disconnected_finite_components_agree() {
    let dist = two_clusters(5, 40.0);
    for threads in [1usize, 4] {
        for threshold in [1.5, 3.0] {
            let params = RipsParams::new(2)
                .with_threads(threads)
                .with_threshold(threshold);
            assert_engines_agree(
                &dist,
                &params,
                &format!("clusters, T {threshold}, {threads}"),
            );
        }
    }
}

// The collapse pipeline owns its own graph, so the engine setting must not
// reach it and must not change what it produces.
#[test]
fn the_collapse_pipeline_ignores_the_engine_setting() {
    let dist = grid(6);
    let base = RipsParams::new(2).with_edge_collapse();
    let expected = rips_persistence(&dist, &base).unwrap();
    for engine in [Engine::Auto, Engine::Dense, Engine::Sparse] {
        for storage in [
            DenseStorage::Auto,
            DenseStorage::Compact,
            DenseStorage::Square,
        ] {
            let got = rips_persistence(
                &dist,
                &base.clone().with_engine(engine).with_dense_storage(storage),
            )
            .unwrap();
            assert_eq!(bits(&got), bits(&expected), "{engine:?}, {storage:?}");
        }
    }
}

#[test]
fn invalid_parameters_fail_the_same_way_under_every_engine() {
    let dist = grid(4);
    for engine in [Engine::Auto, Engine::Dense, Engine::Sparse] {
        let bad_threshold = RipsParams::new(1).with_engine(engine).with_threshold(-1.0);
        assert!(
            rips_persistence(&dist, &bad_threshold).is_err(),
            "{engine:?}"
        );
        let bad_modulus = RipsParams::new(1).with_engine(engine).with_modulus(4);
        assert!(rips_persistence(&dist, &bad_modulus).is_err(), "{engine:?}");
    }
}
