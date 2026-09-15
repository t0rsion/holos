use super::api::cohomology_space;
use super::complex::{ActiveComplex, space_from_complex, space_from_complex_nullspace};
use super::digest::active_graph_digest;
use super::forest::forest_cocycles;
use super::model::CohomologyLimits;
use crate::SparseDistanceMatrix;

fn assert_matches_reference(graph: &SparseDistanceMatrix, scale: f64, modulus: u32) {
    let limits = CohomologyLimits::default();
    let active_edges = graph
        .edges()
        .filter(|&(_, _, value)| value <= scale)
        .map(|(u, v, _)| (u, v))
        .collect::<Vec<_>>();
    let digest = active_graph_digest(graph, scale);
    let fast = space_from_complex(
        ActiveComplex::build(graph.len(), 1, &active_edges, limits).unwrap(),
        graph.len(),
        1,
        scale,
        modulus,
        digest,
        limits,
    )
    .unwrap();
    let reference = space_from_complex_nullspace(
        ActiveComplex::build(graph.len(), 1, &active_edges, limits).unwrap(),
        graph.len(),
        1,
        scale,
        modulus,
        digest,
        limits,
    )
    .unwrap();
    let public = cohomology_space(graph, 1, scale, modulus, limits).unwrap();

    assert_eq!(fast.basis_vectors, reference.basis_vectors);
    assert_eq!(fast.coboundaries, reference.coboundaries);
    assert_eq!(fast.simplices, reference.simplices);
    assert_eq!(fast.simplex_counts, reference.simplex_counts);
    assert_eq!(fast.id, reference.id);
    assert_eq!(fast.basis, reference.basis);
    assert_eq!(public.id, reference.id);
    assert_eq!(public.basis, reference.basis);
}

fn graph_from_mask(vertex_count: usize, mask: u64, weight: f64) -> SparseDistanceMatrix {
    let mut bit = 0;
    let mut edges = Vec::new();
    for u in 0..vertex_count {
        for v in (u + 1)..vertex_count {
            if mask & (1 << bit) != 0 {
                edges.push((u, v, weight));
            }
            bit += 1;
        }
    }
    SparseDistanceMatrix::from_triplets(vertex_count, &edges).unwrap()
}

#[test]
fn forest_matches_unrestricted_nullspace_on_all_small_graphs() {
    for vertex_count in 0usize..=5 {
        let pair_count = vertex_count * vertex_count.saturating_sub(1) / 2;
        for mask in 0..(1u64 << pair_count) {
            let graph = graph_from_mask(vertex_count, mask, 1.0);
            for modulus in [2, 3, 5] {
                assert_matches_reference(&graph, 1.0, modulus);
            }
        }
    }
}

fn weighted_fixture() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        9,
        &[
            (0, 1, 0.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 2.0),
            (0, 2, 1.0),
            (3, 4, 0.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (3, 6, 2.0),
            (4, 6, 1.0),
            (6, 7, 0.0),
            (7, 8, 0.0),
        ],
    )
    .unwrap()
}

#[test]
fn forest_matches_at_weight_ties_and_zero_scales() {
    let graph = weighted_fixture();
    for scale in [-0.0, 0.0, 1.0, 2.0] {
        for modulus in [2, 3, 5] {
            assert_matches_reference(&graph, scale, modulus);
        }
    }
}

fn larger_fixture() -> SparseDistanceMatrix {
    let vertex_count = 36;
    let edges = (0..vertex_count)
        .flat_map(|u| ((u + 1)..vertex_count).map(move |v| (u, v)))
        .filter(|&(u, v)| {
            v == u + 1
                || (v == u + 5 && u % 2 == 0)
                || (v == u + 7 && u % 3 == 0)
                || (u * 11 + v * 7) % 29 == 0
        })
        .map(|(u, v)| (u, v, 1.0))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(vertex_count, &edges).unwrap()
}

#[test]
fn forest_matches_on_larger_sparse_and_triangle_fixture() {
    let graph = larger_fixture();
    for modulus in [2, 3, 5] {
        assert_matches_reference(&graph, 1.0, modulus);
    }
}

#[test]
fn forest_rejects_malformed_incidence() {
    assert!(forest_cocycles(3, &[vec![0, 0]], &[], 5).is_err());
    assert!(forest_cocycles(3, &[vec![0, 1]], &[vec![0, 1, 2]], 5).is_err());
    assert!(
        forest_cocycles(
            3,
            &[vec![0, 1], vec![0, 2], vec![1, 2]],
            &[vec![0, 2, 1]],
            5
        )
        .is_err()
    );
}

#[test]
fn maximal_dimension_is_checked_before_simplex_loop() {
    let graph = SparseDistanceMatrix::from_triplets(1, &[]).unwrap();
    let limits = CohomologyLimits {
        max_dimension: usize::MAX,
        ..CohomologyLimits::default()
    };

    assert!(cohomology_space(&graph, usize::MAX, 0.0, 2, limits).is_err());
    assert!(cohomology_space(&graph, usize::MAX - 1, 0.0, 2, limits).is_err());
    assert!(cohomology_space(&graph, 0, 0.0, 2, limits).is_ok());
}
