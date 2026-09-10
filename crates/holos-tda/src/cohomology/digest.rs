use std::fmt;

use sha2::{Digest, Sha256};

use crate::SparseDistanceMatrix;

use super::model::*;

pub(crate) fn active_graph_digest(graph: &SparseDistanceMatrix, scale: f64) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-active-flag-graph-v1");
    hash.update((graph.len() as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    for (u, v, _) in graph.edges().filter(|edge| edge.2 <= scale) {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
    }
    hash.finalize().into()
}

pub(crate) fn common_graph_digest(
    old: &SparseDistanceMatrix,
    new: &SparseDistanceMatrix,
    scale: f64,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-common-active-flag-graph-v1");
    hash.update((old.len() as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    for (u, v, _) in old
        .edges()
        .filter(|&(u, v, value)| value <= scale && new.get(u, v) <= scale)
    {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
    }
    hash.finalize().into()
}

pub(crate) fn space_id(
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    graph_digest: &[u8; 32],
    simplices: &[Vec<usize>],
    basis: &[SparseVector],
) -> CohomologySpaceId {
    let mut hash = Sha256::new();
    hash.update(b"holos-cohomology-space-v1");
    hash.update((vertex_count as u64).to_be_bytes());
    hash.update((dimension as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    hash.update(modulus.to_be_bytes());
    hash.update(graph_digest);
    hash.update((basis.len() as u64).to_be_bytes());
    for vector in basis {
        hash.update((vector.0.len() as u64).to_be_bytes());
        for (&position, &coefficient) in &vector.0 {
            hash.update((simplices[position].len() as u64).to_be_bytes());
            for vertex in &simplices[position] {
                hash.update((*vertex as u64).to_be_bytes());
            }
            hash.update(coefficient.to_be_bytes());
        }
    }
    CohomologySpaceId(hash.finalize().into())
}

pub(crate) fn class_id(
    space: CohomologySpaceId,
    basis_index: usize,
    vector: &SparseVector,
) -> CohomologyClassId {
    let mut hash = Sha256::new();
    hash.update(b"holos-cohomology-class-v1");
    hash.update(space.as_bytes());
    hash.update((basis_index as u64).to_be_bytes());
    hash.update((vector.0.len() as u64).to_be_bytes());
    for (&position, &coefficient) in &vector.0 {
        hash.update((position as u64).to_be_bytes());
        hash.update(coefficient.to_be_bytes());
    }
    CohomologyClassId(hash.finalize().into())
}

pub(crate) fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8; 32]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}
