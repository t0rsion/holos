//! The dim-0 apparent test on adjacency rows against the neighbor-list test:
//! the same diagram, serial and threaded, on tie-heavy random graphs and on
//! point clouds, at every dimension the rows reach.

use holos_tda::{
    DistanceMatrix, RipsParams, SparseDistanceMatrix, rips_persistence, rips_persistence_sparse,
};

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A dense-enough random graph with a small distance palette, so many
/// triangles tie their edges and the facet order decides the pairs.
fn tie_graph(rng: &mut Rng, n: usize, keep: usize) -> (SparseDistanceMatrix, DistanceMatrix) {
    let palette = [0.5, 1.0, 1.0, 1.5, 2.0, 2.0, 3.0];
    let mut triplets = Vec::new();
    let mut condensed = Vec::new();
    for i in 1..n {
        for j in 0..i {
            if rng.below(4) < keep {
                let w = palette[rng.below(palette.len())];
                triplets.push((i, j, w));
                condensed.push(w);
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

fn cloud(rng: &mut Rng, n: usize, dim: usize) -> DistanceMatrix {
    let points: Vec<Vec<f64>> = (0..n)
        .map(|_| (0..dim).map(|_| rng.uniform()).collect())
        .collect();
    DistanceMatrix::from_points(&points).unwrap()
}

fn bars(mut diagram: holos_tda::Diagram) -> Vec<(usize, u64, u64)> {
    diagram.canonicalize();
    diagram
        .bars
        .iter()
        .map(|b| (b.dim, b.birth.to_bits(), b.death.to_bits()))
        .collect()
}

fn params(max_dim: usize, threshold: Option<f64>, threads: usize, rows: bool) -> RipsParams {
    let mut params = RipsParams::default();
    params.max_dim = max_dim;
    params.threshold = threshold;
    params.threads = threads;
    params.use_adjacency_rows = rows;
    params
}

#[test]
fn rows_give_the_neighbor_list_diagram_on_tie_graphs() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..8 {
        let n = 20 + case * 6;
        let keep = 2 + case % 3;
        let (sparse, dense) = tie_graph(&mut rng, n, keep);
        for max_dim in [1, 2] {
            for threads in [1, 3] {
                let plain =
                    rips_persistence_sparse(&sparse, &params(max_dim, None, threads, false))
                        .unwrap();
                let rows = rips_persistence_sparse(&sparse, &params(max_dim, None, threads, true))
                    .unwrap();
                assert_eq!(
                    bars(rows),
                    bars(plain),
                    "sparse n={n} keep={keep} max_dim={max_dim} threads={threads}"
                );
                let plain =
                    rips_persistence(&dense, &params(max_dim, Some(2.0), threads, false)).unwrap();
                let rows =
                    rips_persistence(&dense, &params(max_dim, Some(2.0), threads, true)).unwrap();
                assert_eq!(
                    bars(rows),
                    bars(plain),
                    "dense n={n} keep={keep} max_dim={max_dim} threads={threads}"
                );
            }
        }
    }
}

#[test]
fn rows_give_the_neighbor_list_diagram_on_clouds() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for case in 0..4 {
        let n = 60 + case * 30;
        let dense = cloud(&mut rng, n, 2 + case % 2);
        for threshold in [None, Some(0.35)] {
            for threads in [1, 4] {
                let plain =
                    rips_persistence(&dense, &params(1, threshold, threads, false)).unwrap();
                let rows = rips_persistence(&dense, &params(1, threshold, threads, true)).unwrap();
                assert_eq!(
                    bars(rows),
                    bars(plain),
                    "cloud n={n} threshold={threshold:?} threads={threads}"
                );
            }
        }
    }
}
