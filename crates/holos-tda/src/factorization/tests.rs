use super::*;
use crate::{RipsParams, rips_persistence_sparse};

#[test]
fn factorization_is_off_by_default() {
    assert_eq!(RipsParams::default().factorization, GraphFactorization::Off);
}

fn graph(n: usize, pairs: &[(usize, usize)]) -> SparseDistanceMatrix {
    let edges: Vec<_> = pairs
        .iter()
        .enumerate()
        .map(|(i, &(u, v))| (u, v, 1.0 + (i % 3) as f64))
        .collect();
    SparseDistanceMatrix::from_triplets(n, &edges).unwrap()
}

#[test]
fn blocks_partition_edges_and_classify_bridges() {
    let matrix = graph(
        8,
        &[
            (0, 1),
            (1, 2),
            (2, 0),
            (2, 3),
            (3, 4),
            (4, 2),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 5),
        ],
    );
    let summary = analyze(&matrix, None).unwrap();
    assert_eq!(summary.blocks, 4);
    assert_eq!(summary.cyclic_blocks, 3);
    assert_eq!(summary.bridge_edges, 1);
    assert_eq!(summary.cyclic_edges, 9);
    assert_eq!(summary.largest_cyclic_block_edges, 3);
}

#[test]
fn forced_factorization_matches_whole_graph_over_fields_and_threads() {
    let mut pairs = Vec::new();
    for offset in [0usize, 4, 8] {
        pairs.extend([
            (offset, offset + 1),
            (offset + 1, offset + 2),
            (offset + 2, offset + 3),
            (offset, offset + 3),
        ]);
    }
    pairs.extend([(3, 4), (7, 8)]);
    let matrix = graph(12, &pairs);
    for modulus in [2, 3, 5] {
        for threads in [1, 3] {
            let mut whole = RipsParams::new(2).with_modulus(modulus);
            whole.threads = threads;
            whole.factorization = GraphFactorization::Off;
            let mut split = whole.clone();
            split.factorization = GraphFactorization::Force;
            assert_eq!(
                rips_persistence_sparse(&matrix, &whole).unwrap().bars,
                rips_persistence_sparse(&matrix, &split).unwrap().bars,
                "modulus {modulus}, threads {threads}"
            );
        }
    }
}

#[test]
fn a_deep_path_does_not_recurse() {
    let pairs: Vec<_> = (0..20_000).map(|u| (u, u + 1)).collect();
    let matrix = graph(20_001, &pairs);
    let summary = analyze(&matrix, None).unwrap();
    assert_eq!(summary.blocks, 20_000);
    assert_eq!(summary.bridge_edges, 20_000);
    assert_eq!(summary.cyclic_blocks, 0);
}

#[test]
fn random_graphs_match_whole_reduction_and_keep_cliques_in_one_block() {
    let mut state = 0x82af_137c_d095_4e61u64;
    for case in 0..80 {
        let matrix = random_graph(&mut state);
        for threshold in [1.0, 3.0] {
            check_random_decomposition(&matrix, case, threshold);
            check_random_persistence(&matrix, case, threshold);
        }
    }
}

fn random_graph(state: &mut u64) -> SparseDistanceMatrix {
    let n = 5 + next_random(state) as usize % 10;
    let mut triplets = Vec::new();
    for u in 0..n {
        for v in u + 1..n {
            if next_random(state) % 5 < 2 {
                triplets.push((u, v, (next_random(state) % 4) as f64));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn check_random_decomposition(matrix: &SparseDistanceMatrix, case: usize, threshold: f64) {
    let decomposition = decompose(matrix, threshold);
    let mut owner = vec![usize::MAX; decomposition.edges.len()];
    for (block, edge_ids) in decomposition.blocks.iter().enumerate() {
        for &edge in edge_ids {
            assert_eq!(owner[edge], usize::MAX, "case {case}: repeated edge");
            owner[edge] = block;
        }
    }
    assert!(owner.iter().all(|&block| block != usize::MAX));
    check_triangle_owners(matrix.len(), &decomposition, &owner, case);
}

fn check_triangle_owners(
    vertices: usize,
    decomposition: &decomposition::Decomposition,
    owner: &[usize],
    case: usize,
) {
    for a in 0..vertices {
        for b in a + 1..vertices {
            for c in b + 1..vertices {
                check_triangle_owner(decomposition, owner, [a, b, c], case);
            }
        }
    }
}

fn check_triangle_owner(
    decomposition: &decomposition::Decomposition,
    owner: &[usize],
    vertices: [usize; 3],
    case: usize,
) {
    let edge = |u: usize, v: usize| {
        decomposition
            .edges
            .iter()
            .position(|item| item.u == u && item.v == v)
    };
    let [a, b, c] = vertices;
    if let (Some(ab), Some(ac), Some(bc)) = (edge(a, b), edge(a, c), edge(b, c)) {
        assert_eq!(owner[ab], owner[ac], "case {case}: triangle");
        assert_eq!(owner[ab], owner[bc], "case {case}: triangle");
    }
}

fn check_random_persistence(matrix: &SparseDistanceMatrix, case: usize, threshold: f64) {
    for modulus in [2, 3] {
        let mut whole = RipsParams::new(2)
            .with_modulus(modulus)
            .with_threshold(threshold);
        whole.factorization = GraphFactorization::Off;
        let mut split = whole.clone();
        split.factorization = GraphFactorization::Force;
        assert_eq!(
            rips_persistence_sparse(matrix, &whole).unwrap().bars,
            rips_persistence_sparse(matrix, &split).unwrap().bars,
            "case {case}, threshold {threshold}, modulus {modulus}"
        );
    }
}
