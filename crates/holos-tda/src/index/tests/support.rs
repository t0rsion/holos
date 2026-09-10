use super::*;

pub(super) fn shared_edge_graph(separator_weight: f64) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, separator_weight),
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
    .unwrap()
}

pub(super) fn compose_index_params() -> IndexParams {
    IndexParams {
        interface_policy: InterfacePolicy::Compose,
        ..IndexParams::default()
    }
}

pub(super) fn joined_octahedra(changed: bool) -> SparseDistanceMatrix {
    let atoms = [[0, 1, 2, 3, 4, 5], [0, 6, 7, 8, 9, 10]];
    let mut edges = Vec::new();
    for vertices in atoms {
        let opposite = [
            EdgeKey::new(vertices[0], vertices[1]),
            EdgeKey::new(vertices[2], vertices[3]),
            EdgeKey::new(vertices[4], vertices[5]),
        ];
        for left in 0..vertices.len() {
            for right in left + 1..vertices.len() {
                let edge = EdgeKey::new(vertices[left], vertices[right]);
                if !opposite.contains(&edge) {
                    let offset = if changed && edge == EdgeKey::new(0, 2) {
                        0.0001
                    } else {
                        0.0
                    };
                    edges.push((
                        edge.u,
                        edge.v,
                        1.0 + (edge.u + edge.v) as f64 / 100.0 + offset,
                    ));
                }
            }
        }
    }
    edges.sort_by_key(|&(u, v, _)| (u, v));
    SparseDistanceMatrix::from_triplets(11, &edges).unwrap()
}

pub(super) fn zero_cone_cover(nonzero_cone_edge: bool) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, if nonzero_cone_edge { 0.1 } else { 0.0 }),
            (0, 2, 0.0),
            (0, 3, 1.0),
            (1, 3, 1.1),
            (2, 3, 1.2),
            (0, 4, 1.3),
            (1, 4, 1.4),
            (2, 4, 1.5),
        ],
    )
    .unwrap()
}
