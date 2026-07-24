//! Known-topology fixtures. Betti-style assertions are kept coarse enough to
//! be robust to sampling; the oracle comparison is the exact check.

use std::f64::consts::PI;

use holos_tda::oracle::rips_persistence_oracle;
use holos_tda::{rips_persistence, Bar, Diagram, DistanceMatrix, RipsParams};

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

    fn gaussian(&mut self) -> f64 {
        let u1 = (self.uniform()).max(1e-12);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
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

fn compute(dist: &DistanceMatrix, max_dim: usize) -> Diagram {
    rips_persistence(dist, &RipsParams::new(max_dim)).unwrap()
}

fn compute_thresholded(dist: &DistanceMatrix, max_dim: usize, threshold: f64) -> Diagram {
    rips_persistence(dist, &RipsParams::new(max_dim).with_threshold(threshold)).unwrap()
}

fn assert_matches_oracle(diagram: &Diagram, dist: &DistanceMatrix, max_dim: usize) {
    let expected = rips_persistence_oracle(dist, max_dim, None);
    assert_eq!(canonical(diagram), canonical(&expected));
}

fn assert_matches_oracle_thresholded(
    diagram: &Diagram,
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: f64,
) {
    let expected = rips_persistence_oracle(dist, max_dim, Some(threshold));
    assert_eq!(canonical(diagram), canonical(&expected));
}

// Bars alive at t under the [birth, death) convention.
fn betti(d: &Diagram, dim: usize, t: f64) -> usize {
    d.in_dim(dim)
        .filter(|b| b.birth <= t && b.death > t)
        .count()
}

fn essential_count(d: &Diagram, dim: usize) -> usize {
    d.in_dim(dim).filter(|b| b.is_essential()).count()
}

fn persistence(b: &Bar) -> f64 {
    b.death - b.birth
}

fn circle_points(n: usize, center: (f64, f64), radius: f64) -> Vec<Vec<f64>> {
    (0..n)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / n as f64;
            vec![center.0 + radius * t.cos(), center.1 + radius * t.sin()]
        })
        .collect()
}

#[test]
fn circle_has_exactly_one_long_h1_class() {
    let dist = DistanceMatrix::from_points(&circle_points(20, (0.0, 0.0), 1.0)).unwrap();
    let diagram = compute(&dist, 1);

    assert_eq!(essential_count(&diagram, 0), 1);
    let long: Vec<&Bar> = diagram
        .in_dim(1)
        .filter(|b| b.death / b.birth > 2.0)
        .collect();
    assert_eq!(long.len(), 1, "expected exactly one dominant H1 bar");

    assert_matches_oracle(&diagram, &dist, 1);
}

#[test]
fn two_disjoint_circles_have_two_components_and_two_loops() {
    // Unit circles centered 10 apart; threshold 2.5 covers each circle's
    // full H1 lifetime but stays far below the 8.0 gap between them.
    let mut points = circle_points(16, (0.0, 0.0), 1.0);
    points.extend(circle_points(16, (10.0, 0.0), 1.0));
    let dist = DistanceMatrix::from_points(&points).unwrap();
    let diagram = compute_thresholded(&dist, 1, 2.5);

    assert_eq!(essential_count(&diagram, 0), 2);
    let long = diagram
        .in_dim(1)
        .filter(|b| b.death / b.birth > 2.0)
        .count();
    assert_eq!(long, 2, "expected exactly two dominant H1 bars");

    assert_matches_oracle_thresholded(&diagram, &dist, 1, 2.5);
}

#[test]
fn circle_betti_numbers_at_prescribed_thresholds() {
    // 20 points on the unit circle: pairwise distances are 2 sin(pi k / 20),
    // k = 1..10, i.e. 0.3129, 0.6180, 0.9080, 1.1756, ... The single H1
    // class is born with the k=1 edges and dies at 2 sin(7 pi/20) = 1.7820
    // (steps up to 6 of 20 still yield a circle; 7/20 >= 1/3 fills it).
    // Probe values sit in the open gaps between those scales.
    let dist = DistanceMatrix::from_points(&circle_points(20, (0.0, 0.0), 1.0)).unwrap();
    let diagram = compute(&dist, 1);

    assert_eq!(betti(&diagram, 0, 0.2), 20);
    assert_eq!(betti(&diagram, 1, 0.2), 0);

    for t in [0.45, 1.0, 1.7] {
        assert_eq!(betti(&diagram, 0, t), 1, "b0 at {t}");
        assert_eq!(betti(&diagram, 1, t), 1, "b1 at {t}");
    }

    assert_eq!(betti(&diagram, 0, 1.85), 1);
    assert_eq!(betti(&diagram, 1, 1.85), 0);

    let h1: Vec<_> = diagram.in_dim(1).collect();
    assert_eq!(h1.len(), 1);
    assert!((h1[0].birth - 2.0 * (PI / 20.0).sin()).abs() < 1e-12);
    assert!((h1[0].death - 2.0 * (7.0 * PI / 20.0).sin()).abs() < 1e-12);
}

#[test]
fn figure_eight_has_two_long_h1_classes() {
    // Two unit circles tangent at the origin. The first circle's k=0 sample
    // is exactly (0, 0); the second circle's k=8 sample is overwritten with
    // the same exact coordinates, so the cloud contains a bitwise duplicate
    // of the tangent point (trig roundoff would otherwise leave it off by
    // ~1e-16 in y).
    let mut points = circle_points(16, (-1.0, 0.0), 1.0);
    points.extend(circle_points(16, (1.0, 0.0), 1.0));
    points[16 + 8] = vec![0.0, 0.0];
    assert_eq!(points[0], points[16 + 8]);
    let dist = DistanceMatrix::from_points(&points).unwrap();
    let diagram = compute(&dist, 1);

    assert_eq!(essential_count(&diagram, 0), 1);
    let long = diagram
        .in_dim(1)
        .filter(|b| b.death / b.birth > 2.0)
        .count();
    assert_eq!(long, 2, "expected exactly two dominant H1 bars");

    assert_matches_oracle(&diagram, &dist, 1);
}

#[test]
fn sphere_sample_has_one_dominant_h2_class() {
    let mut rng = Rng::new(2);
    let points: Vec<Vec<f64>> = (0..30)
        .map(|_| loop {
            let g = [rng.gaussian(), rng.gaussian(), rng.gaussian()];
            let norm = g.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 1e-3 {
                break g.iter().map(|x| x / norm).collect();
            }
        })
        .collect();
    let dist = DistanceMatrix::from_points(&points).unwrap();
    let diagram = compute(&dist, 2);

    assert_eq!(essential_count(&diagram, 0), 1);

    let mut h2: Vec<f64> = diagram.in_dim(2).map(persistence).collect();
    h2.sort_by(|a, b| b.total_cmp(a));
    // Calibrated for this seed: H2 is exactly one bar, (1.3737, 1.7065),
    // persistence ~0.33.
    assert_eq!(h2.len(), 1, "expected exactly one H2 bar: {h2:?}");
    assert!(h2[0] > 0.15, "dominant H2 bar too short: {h2:?}");

    // At this sampling density every H1 loop fills well before doubling its
    // birth scale (max death/birth for this seed is ~1.44).
    assert!(diagram.in_dim(1).all(|b| b.death / b.birth < 2.0));

    assert_matches_oracle(&diagram, &dist, 2);
}

#[test]
fn projective_plane_torsion() {
    // Ripser's 13-vertex RP^2 example. H1 = H2 = Z/2, so both are visible
    // exactly at p = 2 and vanish at any odd prime: the one fixture where
    // the coefficient field changes the answer.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/projective_plane.lower_distance_matrix");
    let dist = holos_tda::io::read_lower_distance_matrix(&path).unwrap();
    let compute_mod =
        |p: u32| rips_persistence(&dist, &RipsParams::new(2).with_modulus(p)).unwrap();
    let h = |d: &Diagram, dim: usize| -> Vec<(f64, f64)> {
        d.in_dim(dim).map(|b| (b.birth, b.death)).collect()
    };

    let mod2 = compute_mod(2);
    assert_eq!(h(&mod2, 1), vec![(1.0, 2.0)]);
    assert_eq!(h(&mod2, 2), vec![(1.0, 2.0)]);

    let h0: Vec<_> = canonical(&mod2).into_iter().filter(|b| b.0 == 0).collect();
    for p in [3, 5] {
        let modp = compute_mod(p);
        assert_eq!(h(&modp, 1), vec![], "H1 must vanish at p = {p}");
        assert_eq!(h(&modp, 2), vec![], "H2 must vanish at p = {p}");
        let h0p: Vec<_> = canonical(&modp).into_iter().filter(|b| b.0 == 0).collect();
        assert_eq!(h0p, h0, "H0 must not depend on p");
    }
}

#[test]
fn projective_plane_torsion_under_every_toggle() {
    // The optimization toggles must not interact with the coefficient
    // field: the torsion answer holds for all 8 combinations at each p.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/projective_plane.lower_distance_matrix");
    let dist = holos_tda::io::read_lower_distance_matrix(&path).unwrap();
    for p in [2u32, 3, 5] {
        let baseline = rips_persistence(&dist, &RipsParams::new(2).with_modulus(p)).unwrap();
        for mask in 0u8..8 {
            let mut params = RipsParams::new(2).with_modulus(p);
            params.use_emergent_pairs = mask & 1 != 0;
            params.use_apparent_pairs = mask & 2 != 0;
            params.use_clearing = mask & 4 != 0;
            let d = rips_persistence(&dist, &params).unwrap();
            assert_eq!(
                canonical(&d),
                canonical(&baseline),
                "toggle mask {mask} changes the diagram at p = {p}"
            );
        }
    }
}

#[test]
fn three_clusters_show_two_long_finite_h0_bars() {
    let mut rng = Rng::new(0xc1a5_7e2e_d5ee_d001);
    let centers = [(0.0, 0.0), (10.0, 0.0), (5.0, 8.66)];
    let mut points = Vec::new();
    for &(cx, cy) in &centers {
        for _ in 0..8 {
            points.push(vec![
                cx + 0.5 * (rng.uniform() - 0.5),
                cy + 0.5 * (rng.uniform() - 0.5),
            ]);
        }
    }
    let dist = DistanceMatrix::from_points(&points).unwrap();
    let diagram = compute(&dist, 0);

    // Enclosing radius exceeds the merge scale, so exactly one component
    // survives and the two inter-cluster merges are finite bars.
    assert_eq!(essential_count(&diagram, 0), 1);
    let finite: Vec<&Bar> = diagram.in_dim(0).filter(|b| !b.is_essential()).collect();
    assert_eq!(finite.len(), points.len() - 1);
    let long = finite.iter().filter(|b| b.death > 5.0).count();
    assert_eq!(long, 2, "expected exactly two inter-cluster merges");
    assert!(finite.iter().all(|b| b.death > 5.0 || b.death < 1.0));

    assert_matches_oracle(&diagram, &dist, 0);
}
