//! Sparse input: exact equivalence with the densified matrix (absent pairs
//! as +inf), oracle comparison, construction validation, and a differential
//! against ripser's sparse format.

use std::path::PathBuf;
use std::process::Command;

use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{
    rips_persistence, rips_persistence_sparse, Bar, Diagram, DistanceMatrix, RipsParams,
    SparseDistanceMatrix,
};

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

// A random sparse matrix and its dense counterpart: dropped pairs are +inf.
fn random_pair(rng: &mut Rng, n: usize) -> (SparseDistanceMatrix, DistanceMatrix) {
    let palette = [0.5, 1.0, 1.5, 2.0, 2.5];
    let mut triplets = Vec::new();
    let mut condensed = Vec::new();
    for i in 1..n {
        for j in 0..i {
            if rng.below(3) > 0 {
                let d = palette[rng.below(palette.len())];
                triplets.push((i, j, d));
                condensed.push(d);
            } else {
                condensed.push(f64::INFINITY);
            }
        }
    }
    (
        SparseDistanceMatrix::from_triplets(n, &triplets).unwrap(),
        DistanceMatrix::from_condensed(condensed).unwrap(),
    )
}

// The sparse default is "no threshold". Densify with an explicit infinite
// threshold, so the dense side skips its enclosing-radius default too.
fn dense_threshold(threshold: Option<f64>) -> Option<f64> {
    Some(threshold.unwrap_or(f64::INFINITY))
}

#[test]
fn random_sparse_inputs_match_densified_dense() {
    let mut rng = Rng::new(0x5ba1_5e5e_ed00_0001);
    for case in 0..80 {
        let n = 2 + rng.below(11);
        let (sparse, dense) = random_pair(&mut rng, n);
        let max_dim = rng.below(3).min(n - 2);
        let threshold = if case % 2 == 0 {
            None
        } else {
            Some(0.4 + 2.4 * rng.uniform())
        };
        for p in [2, 3] {
            let mut params = RipsParams::new(max_dim).with_modulus(p);
            params.threshold = threshold;
            let got = rips_persistence_sparse(&sparse, &params).unwrap();
            params.threshold = dense_threshold(threshold);
            let expected = rips_persistence(&dense, &params).unwrap();
            assert_eq!(
                canonical(&got),
                canonical(&expected),
                "case {case}, n {n}, max_dim {max_dim}, modulus {p}"
            );
        }
    }
}

#[test]
fn random_sparse_inputs_match_oracle() {
    let mut rng = Rng::new(0x0dd5_ea11_5eed_0001);
    for case in 0..40 {
        let n = 2 + rng.below(8);
        let (sparse, dense) = random_pair(&mut rng, n);
        let max_dim = rng.below(3).min(n - 2);
        let threshold = if case % 2 == 0 {
            None
        } else {
            Some(0.4 + 2.4 * rng.uniform())
        };
        for p in [2, 3] {
            let mut params = RipsParams::new(max_dim).with_modulus(p);
            params.threshold = threshold;
            let got = rips_persistence_sparse(&sparse, &params).unwrap();
            let expected =
                rips_persistence_oracle_mod(&dense, max_dim, dense_threshold(threshold), p);
            assert_eq!(
                canonical(&got),
                canonical(&expected),
                "case {case}, n {n}, max_dim {max_dim}, modulus {p}"
            );
        }
    }
}

#[test]
fn no_edges_leaves_every_point_isolated() {
    let sparse = SparseDistanceMatrix::from_triplets(5, &[]).unwrap();
    let got = rips_persistence_sparse(&sparse, &RipsParams::new(2)).unwrap();
    assert_eq!(canonical(&got), vec![(0, 0.0, f64::INFINITY); 5]);
}

#[test]
fn single_edge_merges_one_pair() {
    let sparse = SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.5)]).unwrap();
    let got = rips_persistence_sparse(&sparse, &RipsParams::new(1)).unwrap();
    assert_eq!(
        canonical(&got),
        vec![
            (0, 0.0, 1.5),
            (0, 0.0, f64::INFINITY),
            (0, 0.0, f64::INFINITY)
        ]
    );
}

#[test]
fn from_triplets_rejects_invalid_input() {
    assert!(SparseDistanceMatrix::from_triplets(3, &[(0, 3, 1.0)]).is_err());
    assert!(SparseDistanceMatrix::from_triplets(3, &[(3, 0, 1.0)]).is_err());
    assert!(SparseDistanceMatrix::from_triplets(3, &[(1, 1, 1.0)]).is_err());
    assert!(SparseDistanceMatrix::from_triplets(3, &[(0, 1, -1.0)]).is_err());
    assert!(SparseDistanceMatrix::from_triplets(3, &[(0, 1, f64::NAN)]).is_err());
    assert!(SparseDistanceMatrix::from_triplets(3, &[(0, 1, f64::INFINITY)]).is_err());
    assert!(SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 0, 2.0)]).is_err());
    // A repeated pair with the same distance is fine, in either orientation.
    let ok = SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 0, 1.0)]).unwrap();
    assert_eq!(ok.num_edges(), 1);
}

// Differential against ripser's sparse format.

fn ripser_bin() -> Option<String> {
    match std::env::var("RIPSER_BIN") {
        Ok(path) if !path.is_empty() => Some(path),
        _ => {
            println!("skipping: RIPSER_BIN not set");
            None
        }
    }
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

fn write_sparse_file(name: &str, triplets: &[(usize, usize, f64)]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "holos_sparse_diff_{}_{name}.sparse",
        std::process::id()
    ));
    let body = triplets
        .iter()
        .map(|(i, j, d)| format!("{i} {j} {d}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn random_geometric_graph_matches_ripser_sparse() {
    let Some(bin) = ripser_bin() else { return };
    // 60 seeded points in the unit square, edges kept at distance <= 0.4.
    // That density gives every vertex a neighbor and leaves H1 nonempty.
    // Ripser sizes a sparse input purely by the vertex indices it sees.
    let n = 60;
    let mut rng = Rng::new(0x6e0_6e0_6e0);
    let points: Vec<(f64, f64)> = (0..n).map(|_| (rng.uniform(), rng.uniform())).collect();
    let mut triplets = Vec::new();
    let mut degree = vec![0usize; n];
    for i in 1..n {
        for j in 0..i {
            let d =
                ((points[i].0 - points[j].0).powi(2) + (points[i].1 - points[j].1).powi(2)).sqrt();
            if d <= 0.4 {
                triplets.push((i, j, d));
                degree[i] += 1;
                degree[j] += 1;
            }
        }
    }
    assert!(degree.iter().all(|&d| d > 0), "isolated vertex in fixture");

    let sparse = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
    let ours = rips_persistence_sparse(&sparse, &RipsParams::new(2)).unwrap();
    assert!(ours.in_dim(1).count() > 0, "fixture should have H1");

    let path = write_sparse_file("geometric", &triplets);
    let output = Command::new(&bin)
        .args(["--format", "sparse", "--dim", "2"])
        .arg(&path)
        .output()
        .expect("failed to launch ripser");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "ripser failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let theirs = parse_ripser_output(&String::from_utf8_lossy(&output.stdout));
    assert_diagrams_close("geometric", &ours.bars, &theirs, 2);
}
