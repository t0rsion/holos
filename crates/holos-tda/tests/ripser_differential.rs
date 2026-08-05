//! Differential gate against an external ripser binary, driven by the
//! RIPSER_BIN env var. Each test skips, with a note, when the variable is
//! unset. Tolerance is 1e-5 absolute, because ripser computes and prints in
//! f32. Observed agreement on this corpus is ~5e-7.

use std::path::PathBuf;
use std::process::Command;

use holos_tda::{rips_persistence, Bar, DistanceMatrix, RipsParams};

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

fn ripser_bin() -> Option<String> {
    match std::env::var("RIPSER_BIN") {
        Ok(path) if !path.is_empty() => Some(path),
        _ => {
            println!("skipping: RIPSER_BIN not set");
            None
        }
    }
}

// A ripser build with coefficient support (make ripser-coeff). The plain
// binary rejects --modulus.
fn ripser_coeff_bin() -> Option<String> {
    match std::env::var("RIPSER_COEFF_BIN") {
        Ok(path) if !path.is_empty() => Some(path),
        _ => {
            println!("skipping: RIPSER_COEFF_BIN not set");
            None
        }
    }
}

fn condensed(dist: &DistanceMatrix) -> Vec<f64> {
    let n = dist.len();
    let mut out = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            out.push(dist.get(i, j));
        }
    }
    out
}

fn write_lower_distance_file(name: &str, dist: &DistanceMatrix) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "holos_ripser_diff_{}_{name}.lower_distance_matrix",
        std::process::id()
    ));
    let body = condensed(dist)
        .iter()
        .map(|d| format!("{d}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, body).unwrap();
    path
}

fn run_ripser(
    bin: &str,
    name: &str,
    dist: &DistanceMatrix,
    dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> Vec<Bar> {
    let path = write_lower_distance_file(name, dist);
    let mut cmd = Command::new(bin);
    cmd.args(["--format", "lower-distance", "--dim", &dim.to_string()]);
    if let Some(t) = threshold {
        cmd.args(["--threshold", &t.to_string()]);
    }
    if modulus != 2 {
        cmd.args(["--modulus", &modulus.to_string()]);
    }
    let output = cmd.arg(&path).output().expect("failed to launch ripser");
    let _ = std::fs::remove_file(&path);
    if !output.status.success() {
        panic!(
            "ripser failed on {name} ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    parse_ripser_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ripser_output(stdout: &str) -> Vec<Bar> {
    let mut bars = Vec::new();
    let mut dim = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("persistence intervals in dim ") {
            dim = Some(rest.trim_end_matches(':').parse::<usize>().unwrap());
        } else if let Some(rest) = line.strip_prefix('[') {
            let dim = dim.expect("interval line before any dim header");
            let body = rest.strip_suffix(')').expect("malformed interval line");
            let (birth, death) = body.split_once(',').expect("malformed interval line");
            let death = death.trim();
            bars.push(Bar {
                dim,
                birth: birth.trim().parse().unwrap(),
                death: if death.is_empty() {
                    f64::INFINITY
                } else {
                    death.parse().unwrap()
                },
            });
        }
    }
    bars
}

fn close(a: f64, b: f64) -> bool {
    (a.is_infinite() && b.is_infinite()) || (a - b).abs() <= 1e-5
}

fn assert_diagrams_close(name: &str, ours: &[Bar], ripser: &[Bar], max_dim: usize) {
    for dim in 0..=max_dim {
        let select = |bars: &[Bar]| {
            let mut v: Vec<(f64, f64)> = bars
                .iter()
                .filter(|b| b.dim == dim)
                .map(|b| (b.birth, b.death))
                .collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            v
        };
        let (a, b) = (select(ours), select(ripser));
        assert_eq!(
            a.len(),
            b.len(),
            "{name}: bar count mismatch in dim {dim}: ours {a:?} vs ripser {b:?}"
        );
        for (x, y) in a.iter().zip(&b) {
            assert!(
                close(x.0, y.0) && close(x.1, y.1),
                "{name}: bar mismatch in dim {dim}: ours {x:?} vs ripser {y:?}"
            );
        }
    }
}

fn check_against_ripser(
    bin: &str,
    name: &str,
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
) {
    check_against_ripser_mod(bin, name, dist, max_dim, threshold, 2);
}

fn check_against_ripser_mod(
    bin: &str,
    name: &str,
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) {
    let mut params = RipsParams::new(max_dim).with_modulus(modulus);
    params.threshold = threshold;
    let ours = rips_persistence(dist, &params).unwrap();
    let theirs = run_ripser(bin, name, dist, max_dim, threshold, modulus);
    assert_diagrams_close(name, &ours.bars, &theirs, max_dim);
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
fn circle_fixture_matches_ripser() {
    let Some(bin) = ripser_bin() else { return };
    let dist = DistanceMatrix::from_points(&circle_points(20, (0.0, 0.0), 1.0)).unwrap();
    check_against_ripser(&bin, "circle", &dist, 1, None);
}

#[test]
fn sphere_fixture_matches_ripser() {
    let Some(bin) = ripser_bin() else { return };
    // Same fixture as tests/mathematical.rs sphere_sample_has_one_dominant_h2_class.
    let mut rng = Rng::new(2);
    let gaussian = |rng: &mut Rng| {
        let u1 = rng.uniform().max(1e-12);
        let u2 = rng.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    };
    let points: Vec<Vec<f64>> = (0..30)
        .map(|_| loop {
            let g = [gaussian(&mut rng), gaussian(&mut rng), gaussian(&mut rng)];
            let norm = g.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 1e-3 {
                break g.iter().map(|x| x / norm).collect();
            }
        })
        .collect();
    let dist = DistanceMatrix::from_points(&points).unwrap();
    check_against_ripser(&bin, "sphere", &dist, 2, None);
}

// The torus fixture lives in this suite only. The oracle is too slow at the
// sample size a torus needs.
#[test]
fn torus_fixture_matches_ripser() {
    let Some(bin) = ripser_bin() else { return };
    // Flat torus in R^4: (cos u, sin u, cos v, sin v), seeded uniform sample.
    let mut rng = Rng::new(0x7095_7095_7095_7095);
    let points: Vec<Vec<f64>> = (0..120)
        .map(|_| {
            let u = std::f64::consts::TAU * rng.uniform();
            let v = std::f64::consts::TAU * rng.uniform();
            vec![u.cos(), u.sin(), v.cos(), v.sin()]
        })
        .collect();
    let dist = DistanceMatrix::from_points(&points).unwrap();
    // Distances span [0, 2*sqrt(2)]. At 2.0 both dominant H1 classes have
    // died (deaths ~1.76, ~1.78) and the H2 class is a finite bar
    // (~1.51, ~1.93). The dim-2 reduction also stays quick in debug.
    check_against_ripser(&bin, "torus", &dist, 2, Some(2.0));
}

#[test]
fn random_point_clouds_match_ripser() {
    let Some(bin) = ripser_bin() else { return };
    let mut rng = Rng::new(0x21b5_ee2d_1ff0_c10d);
    for case in 0..10 {
        let max_dim = 1 + case % 2;
        // Keep dim-2 cases small enough that debug-build runs stay quick.
        let n = if max_dim == 2 {
            10 + rng.below(26)
        } else {
            10 + rng.below(51)
        };
        let points: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..3).map(|_| 2.0 * rng.uniform() - 1.0).collect())
            .collect();
        let dist = DistanceMatrix::from_points(&points).unwrap();
        let threshold = if case % 3 == 0 {
            Some(0.8 + rng.uniform())
        } else {
            None
        };
        check_against_ripser(&bin, &format!("cloud_{case}"), &dist, max_dim, threshold);
    }
}

// Replays the random_point_clouds_match_ripser stream and reruns a few of
// the smaller cases at modulus 3 against a coefficient-enabled ripser.
#[test]
fn random_point_clouds_match_ripser_mod_3() {
    let Some(bin) = ripser_coeff_bin() else {
        return;
    };
    let mut rng = Rng::new(0x21b5_ee2d_1ff0_c10d);
    for case in 0..10 {
        let max_dim = 1 + case % 2;
        let n = if max_dim == 2 {
            10 + rng.below(26)
        } else {
            10 + rng.below(51)
        };
        let points: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..3).map(|_| 2.0 * rng.uniform() - 1.0).collect())
            .collect();
        let threshold = if case % 3 == 0 {
            Some(0.8 + rng.uniform())
        } else {
            None
        };
        // The three smallest clouds: n 23 at dim 1, n 13 and n 15 at dim 2.
        if !matches!(case, 4 | 5 | 9) {
            continue;
        }
        let dist = DistanceMatrix::from_points(&points).unwrap();
        check_against_ripser_mod(
            &bin,
            &format!("cloud_{case}_mod3"),
            &dist,
            max_dim,
            threshold,
            3,
        );
    }
}

#[test]
fn random_nonmetric_matrices_match_ripser() {
    let Some(bin) = ripser_bin() else { return };
    let mut rng = Rng::new(0x0451_0451_0451_0451);
    for (case, (n, max_dim)) in [(15usize, 1usize), (20, 2)].into_iter().enumerate() {
        // Uniform random symmetric entries: triangle inequality violated
        // almost surely.
        let data: Vec<f64> = (0..n * (n - 1) / 2)
            .map(|_| 0.1 + 1.9 * rng.uniform())
            .collect();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        check_against_ripser(&bin, &format!("nonmetric_{case}"), &dist, max_dim, None);
    }
}
