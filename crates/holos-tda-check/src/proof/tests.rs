use std::collections::BTreeMap;

use super::graph::{FilteredComplex, check_reduction};
use super::*;

fn square_proof() -> ProofBundle {
    let edges = vec![
        ProofEdge {
            u: 0,
            v: 1,
            value: 1.0,
        },
        ProofEdge {
            u: 0,
            v: 2,
            value: 2.0,
        },
        ProofEdge {
            u: 0,
            v: 3,
            value: 1.0,
        },
        ProofEdge {
            u: 1,
            v: 2,
            value: 1.0,
        },
        ProofEdge {
            u: 1,
            v: 3,
            value: 2.0,
        },
        ProofEdge {
            u: 2,
            v: 3,
            value: 1.0,
        },
    ];
    let graph = Graph::new(4, &edges).unwrap();
    let complex = FilteredComplex::build(&graph, None).unwrap();
    let edge_columns = reference_reduce(&complex.edge_boundaries(2), 2);
    let triangle_columns = reference_reduce(&complex.triangle_boundaries(2), 2);
    let node = AtomProof::new(
        vec![0, 1, 2, 3],
        edges.iter().map(|edge| [edge.u, edge.v]).collect(),
        edge_columns,
        triangle_columns,
    )
    .unwrap();
    let checked = check_reduction(
        &graph,
        None,
        2,
        node.edge_columns(),
        node.triangle_columns(),
    )
    .unwrap();
    let digest = *node.digest();
    let snapshot = SnapshotProof::new(4, edges, vec![digest], checked.diagram).unwrap();
    ProofBundle::new(2, None, vec![node], vec![snapshot]).unwrap()
}

fn reference_reduce(boundaries: &[SparseColumn], modulus: u32) -> Vec<ProofColumn> {
    let modulus = modulus as u64;
    let mut reduced: Vec<SparseColumn> = Vec::new();
    let mut basis: Vec<SparseColumn> = Vec::new();
    let mut owners: BTreeMap<usize, usize> = BTreeMap::new();
    for (target, boundary) in boundaries.iter().enumerate() {
        let mut column = boundary.clone();
        let mut transform = SparseColumn::default();
        transform.insert(target, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = owners.get(&pivot) else {
                break;
            };
            let owner_coefficient = reduced[owner].pivot().unwrap().1;
            let factor = (modulus
                - coefficient * inverse_mod(owner_coefficient, modulus) % modulus)
                % modulus;
            column.add_scaled(&reduced[owner], factor, modulus);
            transform.add_scaled(&basis[owner], factor, modulus);
        }
        if let Some((pivot, _)) = column.pivot() {
            owners.insert(pivot, target);
        }
        reduced.push(column);
        basis.push(transform);
    }
    basis
        .into_iter()
        .map(|column| ProofColumn {
            terms: column
                .0
                .into_iter()
                .map(|(index, coefficient)| ProofTerm {
                    index,
                    coefficient: coefficient as u32,
                })
                .collect(),
        })
        .collect()
}

fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
    let mut base = value;
    let mut exponent = modulus - 2;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result
}

#[test]
fn proof_round_trips_and_checks() {
    let mut proof = square_proof();
    proof.snapshots.push(proof.snapshots[0].clone());
    let bytes = proof.encode().unwrap();
    let decoded = ProofBundle::decode(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
    let verified = decoded.verify().unwrap();
    assert_eq!(verified.snapshots, 2);
    assert_eq!(verified.unique_nodes, 1);
    assert_eq!(verified.reused_references, 1);
    assert_eq!(verified.cached_references, 1);
    assert_eq!(
        verified.edge_columns_checked,
        proof.nodes[0].edge_columns.len()
    );
    assert_eq!(
        verified.triangle_columns_checked,
        proof.nodes[0].triangle_columns.len()
    );
}

fn node_columns_start(proof: &ProofBundle) -> usize {
    8 + 2
        + 1
        + 4
        + 1
        + 8
        + 8
        + 32
        + 4 * 8
        + proof.nodes[0].vertices.len() * 8
        + proof.nodes[0].edges.len() * 2 * 8
}

fn write_wire_usize(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 8].copy_from_slice(&u64::try_from(value).unwrap().to_be_bytes());
}

fn wire_max_usize() -> usize {
    usize::try_from(u64::MAX).unwrap_or(usize::MAX)
}

fn write_node_count(bytes: &mut [u8], field: usize, value: usize) {
    let node_start = 8 + 2 + 1 + 4 + 1 + 8 + 8;
    let offset = node_start + 32 + field * 8;
    write_wire_usize(bytes, offset, value);
}

#[test]
fn proof_rejects_truncation_before_column_reserve() {
    let proof = square_proof();
    let bytes = proof.encode().unwrap();
    let columns_start = node_columns_start(&proof);
    assert!(ProofBundle::decode(&bytes[..columns_start], ProofLimits::default()).is_err());
}

#[test]
fn proof_rejects_huge_raw_edge_column_count_without_panicking() {
    let mut bytes = square_proof().encode().unwrap();
    write_node_count(&mut bytes, 2, wire_max_usize());
    let limits = ProofLimits {
        max_edges: usize::MAX,
        ..ProofLimits::default()
    };
    let result = std::panic::catch_unwind(|| ProofBundle::decode(&bytes, limits));
    assert!(result.is_ok());
    assert!(result.unwrap().is_err());
}

#[test]
fn proof_rejects_huge_term_count_without_panicking() {
    let proof = square_proof();
    let mut bytes = proof.encode().unwrap();
    write_wire_usize(&mut bytes, node_columns_start(&proof), wire_max_usize());
    let limits = ProofLimits {
        max_terms: usize::MAX,
        ..ProofLimits::default()
    };
    let result = std::panic::catch_unwind(|| ProofBundle::decode(&bytes, limits));
    assert!(result.is_ok());
    assert!(result.unwrap().is_err());
}

#[test]
fn mutation_and_arbitrary_bytes_are_rejected_without_panics() {
    let proof = square_proof();
    let bytes = proof.encode().unwrap();
    for position in [0, 8, bytes.len() / 2, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[position] ^= 0x80;
        let result = std::panic::catch_unwind(|| {
            ProofBundle::decode(&changed, ProofLimits::default()).and_then(|proof| proof.verify())
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
    for length in 0..bytes.len().min(256) {
        let result = std::panic::catch_unwind(|| {
            ProofBundle::decode(&bytes[..length], ProofLimits::default())
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}
