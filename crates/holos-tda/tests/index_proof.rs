use holos_tda::{
    CertificateLimits, IndexDeltaProof, IndexParams, IndexSnapshotProof, PersistenceIndex,
    RipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{IndexProofState, ProofLimits};

fn graph(change: usize) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 0.25),
            (0, 2, 1.0 + change as f64 / 100.0),
            (1, 2, 1.5),
            (0, 3, 1.2),
            (1, 3, 1.7),
            (0, 4, 1.1),
            (1, 4, 1.6),
            (0, 5, 1.3),
            (1, 5, 1.8),
        ],
    )
    .unwrap()
}

fn joined_octahedra(changed: bool) -> SparseDistanceMatrix {
    let atoms = [[0, 1, 2, 3, 4, 5], [0, 6, 7, 8, 9, 10]];
    let mut edges = Vec::new();
    for vertices in atoms {
        let opposite = [
            (vertices[0], vertices[1]),
            (vertices[2], vertices[3]),
            (vertices[4], vertices[5]),
        ];
        for left in 0..vertices.len() {
            for right in left + 1..vertices.len() {
                let edge = (vertices[left], vertices[right]);
                if !opposite.contains(&edge) {
                    let offset = if changed && edge == (0, 2) {
                        0.0001
                    } else {
                        0.0
                    };
                    edges.push((
                        edge.0,
                        edge.1,
                        1.0 + (edge.0 + edge.1) as f64 / 100.0 + offset,
                    ));
                }
            }
        }
    }
    edges.sort_by_key(|&(u, v, _)| (u, v));
    SparseDistanceMatrix::from_triplets(11, &edges).unwrap()
}

fn four_dimensional_cross_polytope() -> SparseDistanceMatrix {
    let edges = (0..8)
        .flat_map(|u| (u + 1..8).map(move |v| (u, v)))
        .filter(|&(u, v)| u / 2 != v / 2)
        .map(|(u, v)| (u, v, 1.0 + (u + v) as f64 / 100.0))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(8, &edges).unwrap()
}

fn relative_flower(changed: bool) -> SparseDistanceMatrix {
    let mut edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    for offset in 0..8 {
        let vertex = 4 + offset;
        let side = offset % 4;
        let next = (side + 1) % 4;
        let weight = 2.0 + offset as f64 / 1_000.0;
        edges.push((
            side,
            vertex,
            weight
                + if changed && offset == 0 {
                    0.000_01
                } else {
                    0.0
                },
        ));
        edges.push((next, vertex, weight));
    }
    SparseDistanceMatrix::from_triplets(12, &edges).unwrap()
}

#[test]
fn independent_checker_verifies_composed_h2_snapshot_and_delta() {
    let params = RipsParams::new(2).with_modulus(5);
    let mut index_params = IndexParams::default();
    index_params.max_separator_width = 1;
    index_params.leaf_vertices = 6;
    let first = PersistenceIndex::compile(
        &joined_octahedra(false),
        &params,
        index_params,
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&joined_octahedra(true)).unwrap().index;
    let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
    let delta = IndexDeltaProof::between(&first, &second).unwrap();
    assert!(snapshot.summary().triangle_columns > 0);
    assert!(snapshot.summary().relative_nodes > 0);
    assert!(snapshot.summary().relative_bytes > 0);

    let snapshot_bytes = snapshot.encode().unwrap();
    let mut dimension_limits = ProofLimits::default();
    dimension_limits.max_dimension = 1;
    assert!(IndexProofState::verify_snapshot(&snapshot_bytes, dimension_limits).is_err());
    let mut simplex_limits = ProofLimits::default();
    simplex_limits.max_triangles = 0;
    assert!(IndexProofState::verify_snapshot(&snapshot_bytes, simplex_limits).is_err());
    let mut old_version = snapshot_bytes.clone();
    old_version[8..10].copy_from_slice(&2u16.to_be_bytes());
    assert!(IndexProofState::verify_snapshot(&old_version, ProofLimits::default()).is_err());

    let (mut state, cold) =
        IndexProofState::verify_snapshot(&snapshot_bytes, ProofLimits::default()).unwrap();
    assert!(cold.triangle_columns_checked > 0);
    assert!(cold.relative_nodes_checked > 0);
    let warm = state
        .apply_delta(&delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    assert!(warm.triangle_columns_checked > 0);
    assert!(warm.relative_nodes_checked > 0);
    assert_eq!(state.root(), &second.version());
    assert_eq!(
        state
            .diagram()
            .iter()
            .filter(|bar| bar.dimension == 2)
            .count(),
        2
    );
}

#[test]
fn relative_delta_binds_a_changed_source_cell() {
    let params = RipsParams::new(2).with_modulus(3);
    let mut index_params = IndexParams::default();
    index_params.leaf_vertices = 6;
    let first = PersistenceIndex::compile(
        &relative_flower(false),
        &params,
        index_params,
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&relative_flower(true)).unwrap().index;
    assert_ne!(first.version(), second.version());
    assert_eq!(first.diagram().bars, second.diagram().bars);

    let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
    let delta = IndexDeltaProof::between(&first, &second).unwrap();
    let (mut state, _) =
        IndexProofState::verify_snapshot(&snapshot.encode().unwrap(), ProofLimits::default())
            .unwrap();
    let checked = state
        .apply_delta(&delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    assert!(checked.relative_nodes_checked > 0);
    assert_eq!(state.root(), &second.version());
}

#[test]
fn independent_checker_derives_h3_from_a_graded_reduction() {
    let graph = four_dimensional_cross_polytope();
    let params = RipsParams::new(3).with_modulus(3);
    let mut index_params = IndexParams::default();
    index_params.max_separator_width = 1;
    index_params.leaf_vertices = 8;
    let index =
        PersistenceIndex::compile(&graph, &params, index_params, CertificateLimits::default())
            .unwrap();
    assert_eq!(index.diagram().in_dim(3).count(), 1);
    let proof = IndexSnapshotProof::from_index(&index)
        .unwrap()
        .encode()
        .unwrap();
    let (state, checked) =
        IndexProofState::verify_snapshot(&proof, ProofLimits::default()).unwrap();
    assert!(checked.higher_columns_checked > 0);
    assert_eq!(
        state
            .diagram()
            .iter()
            .filter(|bar| bar.dimension == 3)
            .count(),
        1
    );
}

#[test]
fn independent_checker_verifies_a_nonclique_zero_cone() {
    let graph = SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, 0.0),
            (0, 2, 0.0),
            (0, 3, 1.0),
            (1, 3, 1.1),
            (2, 3, 1.2),
            (0, 4, 1.3),
            (1, 4, 1.4),
            (2, 4, 1.5),
        ],
    )
    .unwrap();
    let params = RipsParams::new(2).with_modulus(3);
    let mut index_params = IndexParams::default();
    index_params.max_separator_width = 3;
    index_params.leaf_vertices = 4;
    index_params.interface_policy = holos_tda::InterfacePolicy::Compose;
    let index =
        PersistenceIndex::compile(&graph, &params, index_params, CertificateLimits::default())
            .unwrap();
    assert_eq!(
        index.interfaces()[0].mode,
        holos_tda::InterfaceMode::ZeroCone
    );
    let proof = IndexSnapshotProof::from_index(&index)
        .unwrap()
        .encode()
        .unwrap();
    let (state, checked) =
        IndexProofState::verify_snapshot(&proof, ProofLimits::default()).unwrap();
    assert!(checked.composed_nodes_checked > 0);
    assert_eq!(state.diagram().len(), index.diagram().bars.len());

    let nonzero = SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, 0.1),
            (0, 2, 0.0),
            (0, 3, 1.0),
            (1, 3, 1.1),
            (2, 3, 1.2),
            (0, 4, 1.3),
            (1, 4, 1.4),
            (2, 4, 1.5),
        ],
    )
    .unwrap();
    let materialized = index.transition(&nonzero).unwrap().index;
    let delta = IndexDeltaProof::between(&index, &materialized)
        .unwrap()
        .encode()
        .unwrap();
    let (mut state, _) = IndexProofState::verify_snapshot(&proof, ProofLimits::default()).unwrap();
    let checked = state.apply_delta(&delta, ProofLimits::default()).unwrap();
    assert!(checked.nodes_checked > 0);
    assert_eq!(state.root(), &materialized.version());
}

#[test]
fn independent_checker_advances_snapshot_deltas() {
    let params = RipsParams::new(1).with_modulus(5);
    let first = PersistenceIndex::compile(
        &graph(0),
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&graph(1)).unwrap().index;
    let third = second.transition(&graph(2)).unwrap().index;

    let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
    let first_delta = IndexDeltaProof::between(&first, &second).unwrap();
    let second_delta = IndexDeltaProof::between(&second, &third).unwrap();
    assert!(first_delta.summary().nodes < snapshot.summary().nodes);

    let (mut state, cold) =
        IndexProofState::verify_snapshot(&snapshot.encode().unwrap(), ProofLimits::default())
            .unwrap();
    assert_eq!(&cold.root, snapshot.root());
    let first_checked = state
        .apply_delta(&first_delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    assert_eq!(first_checked.edge_changes, 1);
    assert_eq!(state.root(), first_delta.new_root());
    let second_checked = state
        .apply_delta(&second_delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    assert_eq!(second_checked.edge_changes, 1);
    assert_eq!(state.root(), second_delta.new_root());
    assert_eq!(state.diagram().len(), third.diagram().bars.len());
}

#[test]
fn independent_checker_composes_zero_simplex_interfaces() {
    let params = RipsParams::new(1).with_modulus(5);
    let first_graph = SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 0.0),
            (0, 2, 1.0),
            (1, 2, 1.5),
            (0, 3, 1.2),
            (1, 3, 1.7),
            (0, 4, 1.1),
            (1, 4, 1.6),
            (0, 5, 1.3),
            (1, 5, 1.8),
        ],
    )
    .unwrap();
    let second_graph = SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 0.0),
            (0, 2, 1.01),
            (1, 2, 1.5),
            (0, 3, 1.2),
            (1, 3, 1.7),
            (0, 4, 1.1),
            (1, 4, 1.6),
            (0, 5, 1.3),
            (1, 5, 1.8),
        ],
    )
    .unwrap();
    let first = PersistenceIndex::compile(
        &first_graph,
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert!(first.summary().root_composed);
    let second = first.transition(&second_graph).unwrap().index;
    assert!(second.summary().root_composed);

    let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
    let delta = IndexDeltaProof::between(&first, &second).unwrap();
    let (mut state, _) =
        IndexProofState::verify_snapshot(&snapshot.encode().unwrap(), ProofLimits::default())
            .unwrap();
    state
        .apply_delta(&delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    assert_eq!(state.root(), &second.version());
    assert_eq!(state.diagram().len(), second.diagram().bars.len());
}

#[test]
fn independent_checker_accepts_a_return_to_a_known_root() {
    let params = RipsParams::new(1).with_modulus(3);
    let first = PersistenceIndex::compile(
        &graph(0),
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&graph(1)).unwrap().index;
    let third = second.transition(&graph(0)).unwrap().index;
    assert_eq!(first.version(), third.version());

    let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
    let first_delta = IndexDeltaProof::between(&first, &second).unwrap();
    let second_delta = IndexDeltaProof::between(&second, &third).unwrap();
    let (mut state, _) =
        IndexProofState::verify_snapshot(&snapshot.encode().unwrap(), ProofLimits::default())
            .unwrap();
    state
        .apply_delta(&first_delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    state
        .apply_delta(&second_delta.encode().unwrap(), ProofLimits::default())
        .unwrap();
    assert_eq!(state.root(), &first.version());
}

#[test]
fn independent_checker_rejects_mutation_and_truncation() {
    let params = RipsParams::new(1).with_modulus(3);
    let first = PersistenceIndex::compile(
        &graph(0),
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&graph(1)).unwrap().index;
    let snapshot = IndexSnapshotProof::from_index(&first)
        .unwrap()
        .encode()
        .unwrap();
    let delta = IndexDeltaProof::between(&first, &second)
        .unwrap()
        .encode()
        .unwrap();

    for position in [0, 8, snapshot.len() / 2, snapshot.len() - 1] {
        let mut changed = snapshot.clone();
        changed[position] ^= 0x80;
        let result = std::panic::catch_unwind(|| {
            IndexProofState::verify_snapshot(&changed, ProofLimits::default())
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
    for length in 0..delta.len().min(256) {
        let mut state = IndexProofState::verify_snapshot(&snapshot, ProofLimits::default())
            .unwrap()
            .0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            state.apply_delta(&delta[..length], ProofLimits::default())
        }));
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}
